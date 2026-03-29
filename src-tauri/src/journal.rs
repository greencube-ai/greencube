use std::sync::Arc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::memory::episodic;
use crate::knowledge;
use crate::goals;
use crate::agent_messages;
use crate::providers::Provider;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: String,
    pub agent_id: String,
    pub date: String,
    pub content: String,
    pub task_count: i64,
    pub highlights: Option<String>,
    pub created_at: String,
}

pub fn get_latest_journal(conn: &Connection, agent_id: &str) -> anyhow::Result<Option<JournalEntry>> {
    let result = conn.query_row(
        "SELECT id, agent_id, date, content, task_count, highlights, created_at
         FROM journal_entries WHERE agent_id = ?1 ORDER BY date DESC LIMIT 1",
        params![agent_id],
        |row| Ok(JournalEntry {
            id: row.get(0)?, agent_id: row.get(1)?, date: row.get(2)?,
            content: row.get(3)?, task_count: row.get(4)?, highlights: row.get(5)?,
            created_at: row.get(6)?,
        }),
    );
    match result {
        Ok(entry) => Ok(Some(entry)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Check if the agent should write a journal entry now.
/// Triggers: every 3rd task today, or idle thinker after 6PM with no entry today.
pub fn should_write_journal(conn: &Connection, agent_id: &str) -> bool {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    // Check if we already journaled in the last 2 hours
    let last_journal_key = format!("last_journal_{}",agent_id);
    let last_time: String = conn.query_row(
        "SELECT value FROM config_store WHERE key = ?1",
        params![last_journal_key],
        |row| row.get(0),
    ).unwrap_or_default();

    if !last_time.is_empty() {
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(&last_time) {
            let elapsed = chrono::Utc::now().signed_duration_since(parsed.with_timezone(&chrono::Utc));
            if elapsed.num_hours() < 2 {
                return false;
            }
        }
    }

    // Count today's tasks for this agent
    let today_tasks: i64 = conn.query_row(
        "SELECT COUNT(*) FROM episodes WHERE agent_id = ?1 AND event_type = 'task_end' AND created_at LIKE ?2 || '%'",
        params![agent_id, today],
        |row| row.get(0),
    ).unwrap_or(0);

    today_tasks >= 3 && today_tasks % 3 == 0
}

pub fn spawn_journal_synthesis(state: Arc<AppState>, agent_id: String, provider: Provider) {
    tokio::spawn(async move {
        if let Err(e) = write_journal(&state, &agent_id, &provider).await {
            tracing::warn!("Journal writing failed for {}: {}", agent_id, e);
        }
    });
}

async fn write_journal(state: &AppState, agent_id: &str, provider: &Provider) -> anyhow::Result<()> {
    // Budget check
    {
        let db = state.db.lock().await;
        let budget = state.config.read().await.cost.daily_background_token_budget;
        if !crate::token_usage::has_budget_remaining(&db, agent_id, 600, budget)? {
            tracing::info!("Budget exceeded, skipping journal for agent {}", agent_id);
            return Ok(());
        }
    }

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    // Gather today's data (brief DB lock)
    let (episodes, new_knowledge, active_goals, messages_count) = {
        let db = state.db.lock().await;

        // Today's episode summaries
        let mut stmt = db.prepare(
            "SELECT summary FROM episodes WHERE agent_id = ?1 AND created_at LIKE ?2 || '%' ORDER BY created_at"
        )?;
        let eps: Vec<String> = stmt.query_map(params![agent_id, today], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        // Knowledge created today
        let mut stmt2 = db.prepare(
            "SELECT content FROM knowledge WHERE agent_id = ?1 AND created_at LIKE ?2 || '%'"
        )?;
        let know: Vec<String> = stmt2.query_map(params![agent_id, today], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let goals_list = goals::list_goals(&db, agent_id, Some("active"))?;

        let msg_count: i64 = db.query_row(
            "SELECT COUNT(*) FROM messages WHERE (from_agent_id = ?1 OR to_agent_id = ?1) AND created_at LIKE ?2 || '%'",
            params![agent_id, today],
            |row| row.get(0),
        ).unwrap_or(0);

        (eps, know, goals_list, msg_count)
    };
    // DB lock released

    let events_summary = if episodes.is_empty() {
        "No tasks today.".to_string()
    } else {
        episodes.iter().take(15).cloned().collect::<Vec<_>>().join("\n")
    };

    let knowledge_summary = if new_knowledge.is_empty() {
        "Nothing new learned.".to_string()
    } else {
        new_knowledge.iter().map(|k| format!("- {}", k)).collect::<Vec<_>>().join("\n")
    };

    let goals_text = if active_goals.is_empty() {
        "No active goals.".to_string()
    } else {
        active_goals.iter().map(|g| format!("- {}", g.content)).collect::<Vec<_>>().join("\n")
    };

    let messages_text = if messages_count > 0 {
        format!("{} messages exchanged with other agents.", messages_count)
    } else {
        "No agent communications.".to_string()
    };

    let prompt = format!(
        r#"Write a brief journal entry for today. You are writing to yourself — your future self will read this tomorrow to understand where you are.

Today's events:
{}

Knowledge learned today:
{}

Goal progress:
{}

Communications:
{}

Write naturally. Not bullet points. Tell the story of your day.
Include: what went well, what was hard, what's still unfinished.
Be honest (commandment 2). Flag uncertainty (commandment 9).
Max 300 words."#,
        events_summary, knowledge_summary, goals_text, messages_text,
    );

    // LLM call (no DB lock)
    let client = reqwest::Client::new();
    let response = client.post(format!("{}/chat/completions", provider.api_base_url))
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .json(&serde_json::json!({
            "model": provider.default_model,
            "messages": [
                {"role": "system", "content": "You are an AI assistant."},
                {"role": "user", "content": prompt}
            ],
            "max_tokens": 600,
            "temperature": 0.5,
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Journal LLM call failed");
    }

    let body: serde_json::Value = response.json().await?;

    // Record token usage
    let tokens_used = body["usage"]["total_tokens"].as_i64().unwrap_or(600);
    {
        let db = state.db.lock().await;
        let _ = crate::token_usage::record_usage(&db, agent_id, "journal", tokens_used);
    }

    let content = body["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string();

    if content.is_empty() { return Ok(()); }

    // Store journal (brief DB lock)
    {
        let db = state.db.lock().await;
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let task_count: i64 = db.query_row(
            "SELECT COUNT(*) FROM episodes WHERE agent_id = ?1 AND event_type = 'task_end' AND created_at LIKE ?2 || '%'",
            params![agent_id, today],
            |row| row.get(0),
        ).unwrap_or(0);

        db.execute(
            "INSERT INTO journal_entries (id, agent_id, date, content, task_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(agent_id, date) DO UPDATE SET content = ?4, task_count = ?5, created_at = ?6",
            params![id, agent_id, today, content, task_count, now],
        )?;

        // Track last journal time
        let key = format!("last_journal_{}", agent_id);
        db.execute(
            "INSERT INTO config_store (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, now],
        )?;
    }

    if let Some(handle) = &state.app_handle {
        let _ = handle.emit("activity-refresh", ());
    }

    tracing::info!("Journal written for agent {} ({})", agent_id, today);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_get_latest_journal_empty() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        assert!(get_latest_journal(&conn, &agent.id).expect("get").is_none());
    }

    #[test]
    fn test_insert_and_get_journal() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO journal_entries (id, agent_id, date, content, task_count, created_at) VALUES ('j1', ?1, ?2, 'Good day today.', 3, ?3)",
            params![agent.id, today, now],
        ).expect("insert");
        let entry = get_latest_journal(&conn, &agent.id).expect("get").expect("exists");
        assert_eq!(entry.content, "Good day today.");
        assert_eq!(entry.task_count, 3);
    }

    #[test]
    fn test_journal_upsert() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO journal_entries (id, agent_id, date, content, task_count, created_at) VALUES ('j1', ?1, ?2, 'First draft.', 1, ?3)",
            params![agent.id, today, now],
        ).expect("insert");
        conn.execute(
            "INSERT INTO journal_entries (id, agent_id, date, content, task_count, created_at) VALUES ('j2', ?1, ?2, 'Updated draft.', 3, ?3) ON CONFLICT(agent_id, date) DO UPDATE SET content = 'Updated draft.', task_count = 3",
            params![agent.id, today, now],
        ).expect("upsert");
        let entry = get_latest_journal(&conn, &agent.id).expect("get").expect("exists");
        assert_eq!(entry.content, "Updated draft.");
    }
}
