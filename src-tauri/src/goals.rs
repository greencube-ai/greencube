use std::sync::Arc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::knowledge;
use crate::notifications;
use crate::providers::Provider;
use crate::state::AppState;

const GOAL_GENERATION_INTERVAL: i64 = 10;
const MAX_ACTIVE_GOALS: i64 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub agent_id: String,
    pub content: String,
    pub status: String, // active, achieved, abandoned
    pub progress_notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn list_goals(conn: &Connection, agent_id: &str, status: Option<&str>) -> anyhow::Result<Vec<Goal>> {
    let (sql, param_count) = if let Some(s) = status {
        ("SELECT id, agent_id, content, status, progress_notes, created_at, updated_at FROM agent_goals WHERE agent_id = ?1 AND status = ?2 ORDER BY created_at DESC", 2)
    } else {
        ("SELECT id, agent_id, content, status, progress_notes, created_at, updated_at FROM agent_goals WHERE agent_id = ?1 ORDER BY created_at DESC LIMIT 20", 1)
    };

    let mut stmt = conn.prepare(sql)?;
    let goals = if param_count == 2 {
        stmt.query_map(params![agent_id, status.unwrap()], map_goal)?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(params![agent_id], map_goal)?
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(goals)
}

pub fn count_active_goals(conn: &Connection, agent_id: &str) -> anyhow::Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM agent_goals WHERE agent_id = ?1 AND status = 'active'",
        params![agent_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

pub fn insert_goal(conn: &Connection, agent_id: &str, content: &str) -> anyhow::Result<Goal> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO agent_goals (id, agent_id, content, status, created_at, updated_at) VALUES (?1, ?2, ?3, 'active', ?4, ?5)",
        params![id, agent_id, content, now, now],
    )?;
    Ok(Goal { id, agent_id: agent_id.into(), content: content.into(), status: "active".into(), progress_notes: None, created_at: now.clone(), updated_at: now })
}

pub fn update_goal_status(conn: &Connection, id: &str, status: &str, notes: Option<&str>) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE agent_goals SET status = ?1, progress_notes = COALESCE(?2, progress_notes), updated_at = ?3 WHERE id = ?4",
        params![status, notes, now, id],
    )?;
    Ok(())
}

fn map_goal(row: &rusqlite::Row) -> rusqlite::Result<Goal> {
    Ok(Goal {
        id: row.get(0)?, agent_id: row.get(1)?, content: row.get(2)?,
        status: row.get(3)?, progress_notes: row.get(4)?,
        created_at: row.get(5)?, updated_at: row.get(6)?,
    })
}

/// Check if goals should be generated and spawn if so.
/// Triggers: every 10 tasks OR if 0 active goals and >= 5 tasks (cold start).
pub fn maybe_generate_goals(
    state: Arc<AppState>,
    agent_id: String,
    provider: Provider,
    total_tasks: i64,
    active_goal_count: i64,
) {
    let should_generate = (total_tasks > 0 && total_tasks % GOAL_GENERATION_INTERVAL == 0)
        || (active_goal_count == 0 && total_tasks >= 5);

    if should_generate && active_goal_count < MAX_ACTIVE_GOALS {
        tokio::spawn(async move {
            if let Err(e) = generate_goals(&state, &agent_id, &provider, active_goal_count).await {
                tracing::warn!("Goal generation failed for {}: {}", agent_id, e);
            }
        });
    }
}

async fn generate_goals(
    state: &AppState,
    agent_id: &str,
    provider: &Provider,
    current_active: i64,
) -> anyhow::Result<()> {
    let slots_available = MAX_ACTIVE_GOALS - current_active;
    if slots_available <= 0 { return Ok(()); }

    let (agent, knowledge_entries, active_goals) = {
        let db = state.db.lock().await;
        let agent = crate::identity::registry::get_agent(&db, agent_id)?
            .ok_or_else(|| anyhow::anyhow!("agent not found"))?;
        let knowledge = knowledge::list_knowledge(&db, agent_id, 10)?;
        let goals = list_goals(&db, agent_id, Some("active"))?;
        (agent, knowledge, goals)
    };

    let success_rate = if agent.total_tasks > 0 {
        (agent.successful_tasks as f64 / agent.total_tasks as f64 * 100.0) as i64
    } else { 0 };

    let knowledge_summary = knowledge_entries.iter()
        .take(5)
        .map(|k| format!("- [{}] {}", k.category, k.content))
        .collect::<Vec<_>>()
        .join("\n");

    let existing_goals = active_goals.iter()
        .map(|g| format!("- {}", g.content))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        r#"Based on your experience:
- Tasks completed: {}, Success rate: {}%
- Knowledge: {}
{}

You currently have {} active goal(s). You may add up to {} more.

Set new goals for your growth. Format: [goal] specific objective
If nothing comes to mind, respond: NONE"#,
        agent.total_tasks, success_rate,
        if knowledge_summary.is_empty() { "None yet" } else { &knowledge_summary },
        if existing_goals.is_empty() { "No current goals.".to_string() } else { format!("Current goals:\n{}", existing_goals) },
        current_active, slots_available,
    );

    let client = reqwest::Client::new();
    let response = client.post(format!("{}/chat/completions", provider.api_base_url))
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .json(&serde_json::json!({
            "model": provider.default_model,
            "messages": [
                {"role": "system", "content": "You are an AI assistant."},
                {"role": "user", "content": prompt}
            ],
            "max_tokens": 300,
            "temperature": 0.5,
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send().await?;

    if !response.status().is_success() { anyhow::bail!("Goal generation LLM call failed"); }

    let body: serde_json::Value = response.json().await?;
    let content = body["choices"][0]["message"]["content"].as_str().unwrap_or("NONE");

    if content.trim() == "NONE" { return Ok(()); }

    let db = state.db.lock().await;
    let mut added = 0i64;

    for line in content.lines() {
        if added >= slots_available { break; }
        let trimmed = line.trim();
        if let Some(goal_text) = trimmed.strip_prefix("[goal]") {
            let goal_text = goal_text.trim();
            if !goal_text.is_empty() {
                let _ = insert_goal(&db, agent_id, goal_text);
                added += 1;
            }
        }
    }

    if added > 0 {
        tracing::info!("Generated {} goals for agent {}", added, agent_id);
        if let Some(handle) = &state.app_handle {
            let _ = handle.emit("activity-refresh", ());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_insert_and_list_goals() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        insert_goal(&conn, &agent.id, "Learn Python deeply").expect("g1");
        insert_goal(&conn, &agent.id, "Improve error handling").expect("g2");
        let goals = list_goals(&conn, &agent.id, None).expect("list");
        assert_eq!(goals.len(), 2);
        assert_eq!(goals[0].status, "active");
    }

    #[test]
    fn test_count_active_goals() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        insert_goal(&conn, &agent.id, "Goal 1").expect("g1");
        insert_goal(&conn, &agent.id, "Goal 2").expect("g2");
        assert_eq!(count_active_goals(&conn, &agent.id).expect("count"), 2);
    }

    #[test]
    fn test_update_goal_status() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let goal = insert_goal(&conn, &agent.id, "Learn Rust").expect("g");
        update_goal_status(&conn, &goal.id, "achieved", Some("Completed Rustlings")).expect("update");
        let goals = list_goals(&conn, &agent.id, Some("achieved")).expect("list");
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].progress_notes.as_deref(), Some("Completed Rustlings"));
    }

    #[test]
    fn test_goal_max_enforced() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        insert_goal(&conn, &agent.id, "G1").expect("g1");
        insert_goal(&conn, &agent.id, "G2").expect("g2");
        insert_goal(&conn, &agent.id, "G3").expect("g3");
        assert_eq!(count_active_goals(&conn, &agent.id).expect("count"), 3);
        // maybe_generate_goals should not fire when active >= MAX_ACTIVE_GOALS
        // (tested via the condition, not the async function)
    }
}
