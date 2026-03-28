use std::sync::Arc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::memory::episodic;
use crate::memory::Episode;
use crate::notifications;
use crate::providers;
use crate::state::AppState;

const MAX_RETRIES: i64 = 3;
const MAX_REMINDER_MINUTES: i64 = 10080; // 1 week

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedTask {
    pub id: String,
    pub agent_id: String,
    pub messages_json: String,
    pub provider_id: Option<String>,
    pub resume_at: String,
    pub status: String,
    pub retry_count: i64,
    pub error_info: Option<String>,
    pub source: String,
    pub prompt: Option<String>,
    pub created_at: String,
}

pub fn queue_task(
    conn: &Connection,
    agent_id: &str,
    messages_json: &str,
    provider_id: Option<&str>,
    resume_at: &str,
    source: &str,
    prompt: Option<&str>,
) -> anyhow::Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO task_queue (id, agent_id, messages_json, provider_id, resume_at, status, retry_count, source, prompt, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'queued', 0, ?6, ?7, ?8)",
        params![id, agent_id, messages_json, provider_id, resume_at, source, prompt, now],
    )?;
    Ok(id)
}

fn get_due_tasks(conn: &Connection) -> anyhow::Result<Vec<QueuedTask>> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, messages_json, provider_id, resume_at, status, retry_count, error_info, source, prompt, created_at
         FROM task_queue WHERE status = 'queued' AND resume_at <= ?1 ORDER BY resume_at ASC LIMIT 10"
    )?;
    let tasks = stmt.query_map(params![now], |row| {
        Ok(QueuedTask {
            id: row.get(0)?, agent_id: row.get(1)?, messages_json: row.get(2)?,
            provider_id: row.get(3)?, resume_at: row.get(4)?, status: row.get(5)?,
            retry_count: row.get(6)?, error_info: row.get(7)?, source: row.get(8)?,
            prompt: row.get(9)?, created_at: row.get(10)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(tasks)
}

fn update_status(conn: &Connection, id: &str, status: &str, error: Option<&str>, inc_retry: bool) -> anyhow::Result<()> {
    if inc_retry {
        conn.execute(
            "UPDATE task_queue SET status = ?1, error_info = ?2, retry_count = retry_count + 1 WHERE id = ?3",
            params![status, error, id],
        )?;
    } else {
        conn.execute(
            "UPDATE task_queue SET status = ?1, error_info = ?2 WHERE id = ?3",
            params![status, error, id],
        )?;
    }
    Ok(())
}

/// Clean up old completed/failed tasks (>7 days). Run on startup.
pub fn cleanup_old_tasks(conn: &Connection) -> anyhow::Result<()> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    conn.execute(
        "DELETE FROM task_queue WHERE (status = 'completed' OR status = 'failed') AND created_at < ?1",
        params![cutoff],
    )?;
    Ok(())
}

/// Queue a rate-limited request for auto-retry.
pub fn queue_rate_limited(
    conn: &Connection,
    agent_id: &str,
    messages_json: &str,
    provider_id: Option<&str>,
    retry_after_secs: u64,
) -> anyhow::Result<String> {
    let resume_at = (chrono::Utc::now() + chrono::Duration::seconds(retry_after_secs as i64)).to_rfc3339();
    queue_task(conn, agent_id, messages_json, provider_id, &resume_at, "rate_limit", None)
}

/// Queue a reminder (commitment) from set_reminder tool.
pub fn queue_reminder(
    conn: &Connection,
    agent_id: &str,
    prompt: &str,
    minutes_from_now: i64,
    provider_id: Option<&str>,
) -> anyhow::Result<String> {
    if minutes_from_now > MAX_REMINDER_MINUTES {
        anyhow::bail!("Reminders can't be more than 1 week out (10,080 minutes). Requested: {} minutes.", minutes_from_now);
    }
    if minutes_from_now < 1 {
        anyhow::bail!("Reminder must be at least 1 minute from now.");
    }
    let resume_at = (chrono::Utc::now() + chrono::Duration::minutes(minutes_from_now)).to_rfc3339();
    let messages = serde_json::json!([
        {"role": "system", "content": "You are an AI assistant."},
        {"role": "user", "content": prompt}
    ]);
    queue_task(conn, agent_id, &messages.to_string(), provider_id, &resume_at, "reminder", Some(prompt))
}

/// Background queue processor. Spawned from main.rs.
pub async fn run_queue_processor(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        interval.tick().await;

        // Brief DB lock to get due tasks
        let tasks = {
            let db = state.db.lock().await;
            get_due_tasks(&db).unwrap_or_default()
        };

        for task in tasks {
            // Look up provider (brief lock)
            let provider = {
                let db = state.db.lock().await;
                if let Some(ref pid) = task.provider_id {
                    providers::get_provider(&db, pid).ok().flatten()
                } else {
                    providers::get_default_provider(&db).ok().flatten()
                }
            };

            let provider = match provider {
                Some(p) => p,
                None => {
                    let db = state.db.lock().await;
                    let _ = update_status(&db, &task.id, "failed", Some("No provider found"), false);
                    continue;
                }
            };

            // Check retry limit
            if task.retry_count >= MAX_RETRIES {
                let db = state.db.lock().await;
                let _ = update_status(&db, &task.id, "failed", Some("Max retries exceeded"), false);
                let _ = notifications::create_notification(
                    &db, &task.agent_id,
                    &format!("I tried {} times but kept failing. Task abandoned: {}",
                        MAX_RETRIES, task.prompt.as_deref().unwrap_or("rate-limited request")),
                    "alert", "task_queue"
                );
                if let Some(handle) = &state.app_handle {
                    let _ = handle.emit("notification-new", serde_json::json!({"agent_id": &task.agent_id}));
                    let _ = handle.emit("activity-refresh", ());
                }
                continue;
            }

            // Parse messages (no DB lock held during LLM call)
            let messages: serde_json::Value = match serde_json::from_str(&task.messages_json) {
                Ok(m) => m,
                Err(_) => {
                    let db = state.db.lock().await;
                    let _ = update_status(&db, &task.id, "failed", Some("Invalid messages JSON"), false);
                    continue;
                }
            };

            // Make LLM call
            let client = reqwest::Client::new();
            let response = client
                .post(format!("{}/chat/completions", provider.api_base_url))
                .header("Authorization", format!("Bearer {}", provider.api_key))
                .json(&serde_json::json!({
                    "model": provider.default_model,
                    "messages": messages,
                }))
                .timeout(std::time::Duration::from_secs(120))
                .send()
                .await;

            match response {
                Ok(resp) if resp.status().is_success() => {
                    let body: serde_json::Value = resp.json().await.unwrap_or_default();
                    let content = body["choices"][0]["message"]["content"]
                        .as_str().unwrap_or("Task completed.");

                    let db = state.db.lock().await;
                    let _ = update_status(&db, &task.id, "completed", None, false);

                    // Log episode
                    let _ = episodic::insert_episode(&db, &Episode {
                        id: uuid::Uuid::new_v4().to_string(),
                        agent_id: task.agent_id.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        event_type: if task.source == "reminder" { "commitment_fulfilled" } else { "queued_task_completed" }.into(),
                        summary: format!("{}: {}", task.source, content.chars().take(200).collect::<String>()),
                        raw_data: Some(serde_json::to_string(&body).unwrap_or_default()),
                        task_id: None,
                        outcome: Some("success".into()),
                        tokens_used: 0,
                        cost_cents: 0,
                    });

                    // Notification
                    let notify_msg = if task.source == "reminder" {
                        let relative = crate::time_sense::relative_time(&task.created_at);
                        format!("I committed to \"{}\" {}. Here's what I found: {}",
                            task.prompt.as_deref().unwrap_or("a task"), relative,
                            content.chars().take(200).collect::<String>())
                    } else {
                        format!("I completed the queued task: {}", content.chars().take(200).collect::<String>())
                    };
                    let _ = notifications::create_notification(&db, &task.agent_id, &notify_msg, "achievement", "task_queue");

                    if let Some(handle) = &state.app_handle {
                        let _ = handle.emit("notification-new", serde_json::json!({"agent_id": &task.agent_id}));
                        let _ = handle.emit("activity-refresh", ());
                    }
                    tracing::info!("Queue task completed: {} ({})", task.id, task.source);
                }
                Ok(resp) if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                    // Still rate limited — re-queue with retry
                    let retry_after = resp.headers().get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(60);
                    let new_resume = (chrono::Utc::now() + chrono::Duration::seconds(retry_after as i64)).to_rfc3339();
                    let db = state.db.lock().await;
                    let _ = conn_execute_resume(&db, &task.id, &new_resume);
                    let _ = update_status(&db, &task.id, "queued", Some("Still rate limited"), true);
                    tracing::warn!("Queue task {} still rate limited, retry in {}s", task.id, retry_after);
                }
                Ok(resp) => {
                    let status = resp.status();
                    let error_text = resp.text().await.unwrap_or_default();
                    let db = state.db.lock().await;
                    let _ = update_status(&db, &task.id, "queued", Some(&format!("LLM error: {} {}", status, error_text)), true);
                    // Re-queue in 60 seconds
                    let new_resume = (chrono::Utc::now() + chrono::Duration::seconds(60)).to_rfc3339();
                    let _ = conn_execute_resume(&db, &task.id, &new_resume);
                }
                Err(e) => {
                    let db = state.db.lock().await;
                    let _ = update_status(&db, &task.id, "queued", Some(&format!("Network error: {}", e)), true);
                    let new_resume = (chrono::Utc::now() + chrono::Duration::seconds(60)).to_rfc3339();
                    let _ = conn_execute_resume(&db, &task.id, &new_resume);
                }
            }
        }
    }
}

fn conn_execute_resume(conn: &Connection, id: &str, resume_at: &str) -> anyhow::Result<()> {
    conn.execute("UPDATE task_queue SET resume_at = ?1 WHERE id = ?2", params![resume_at, id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_queue_and_get_due() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let past = (chrono::Utc::now() - chrono::Duration::seconds(10)).to_rfc3339();
        queue_task(&conn, &agent.id, "[]", None, &past, "rate_limit", None).expect("queue");
        let due = get_due_tasks(&conn).expect("get");
        assert_eq!(due.len(), 1);
    }

    #[test]
    fn test_queue_future_not_due() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let future = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        queue_task(&conn, &agent.id, "[]", None, &future, "reminder", Some("check API")).expect("queue");
        let due = get_due_tasks(&conn).expect("get");
        assert_eq!(due.len(), 0);
    }

    #[test]
    fn test_reminder_max_duration() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let result = queue_reminder(&conn, &agent.id, "test", 20000, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("1 week"));
    }

    #[test]
    fn test_cleanup_old_tasks() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let old = (chrono::Utc::now() - chrono::Duration::days(10)).to_rfc3339();
        // Manually insert old completed task
        conn.execute(
            "INSERT INTO task_queue (id, agent_id, messages_json, resume_at, status, retry_count, source, created_at)
             VALUES ('old1', ?1, '[]', ?2, 'completed', 0, 'test', ?2)",
            params![agent.id, old],
        ).expect("insert");
        assert_eq!(get_due_tasks(&conn).expect("get").len(), 0); // Not due (completed)
        cleanup_old_tasks(&conn).expect("cleanup");
        // Verify old task was deleted
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM task_queue", [], |r| r.get(0)).expect("count");
        assert_eq!(count, 0);
    }
}
