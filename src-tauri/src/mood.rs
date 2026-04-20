use rusqlite::{params, Connection};

/// Calculate mood based on recent task history.
// Creature-era module, frozen. See STATUS.md. Retained for potential revival.
#[allow(dead_code)]
pub fn calculate_mood(conn: &Connection, agent_id: &str) -> String {
    // Get last 10 episodes outcomes
    let mut stmt = conn.prepare(
        "SELECT outcome FROM episodes WHERE agent_id = ?1 AND event_type = 'task_end' ORDER BY created_at DESC LIMIT 10"
    ).ok();

    let outcomes: Vec<String> = stmt.as_mut().map(|s| {
        s.query_map(params![agent_id], |row| row.get::<_, String>(0))
            .ok()
            .map(|r| r.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }).unwrap_or_default();

    if outcomes.is_empty() {
        return "neutral".to_string();
    }

    let total = outcomes.len();
    let successes = outcomes.iter().filter(|o| o == &"success").count();
    let success_rate = successes as f64 / total as f64;

    // Check last 5
    let last5 = &outcomes[..total.min(5)];
    let last5_success = last5.iter().filter(|o| *o == &"success").count();
    let last5_rate = last5_success as f64 / last5.len() as f64;

    if last5_rate >= 0.95 {
        "thriving".to_string()
    } else if last5_rate >= 0.7 {
        "learning".to_string()
    } else if last5_rate >= 0.5 {
        "neutral".to_string()
    } else if total >= 10 && success_rate < 0.4 {
        "frustrated".to_string()
    } else {
        "struggling".to_string()
    }
}

/// Get the system prompt injection for the current mood.
// Creature-era module, frozen. See STATUS.md. Retained for potential revival.
#[allow(dead_code)]
pub fn mood_prompt(mood: &str) -> &'static str {
    match mood {
        "thriving" => "\n\nYou've been performing excellently. Feel free to try creative approaches.",
        "learning" => "\n\nYou're improving. Keep building on recent successes.",
        "struggling" => "\n\nYou've had some difficulties recently. Take extra care. Double-check your work.",
        "frustrated" => "\n\nYou've been having a tough time. Slow down. Break problems into smaller steps. It's okay to say you're not sure.",
        _ => "", // neutral = no injection
    }
}

/// Update the agent's mood in the database.
// Creature-era module, frozen. See STATUS.md. Retained for potential revival.
#[allow(dead_code)]
pub fn update_mood(conn: &Connection, agent_id: &str) -> String {
    let mood = calculate_mood(conn, agent_id);
    let _ = conn.execute(
        "UPDATE agents SET status = CASE WHEN status = 'active' THEN 'active' ELSE status END WHERE id = ?1",
        params![agent_id],
    );
    // Store mood in agent_context for easy retrieval
    let _ = conn.execute(
        "INSERT OR REPLACE INTO config_store (key, value) VALUES (?1, ?2)",
        params![format!("mood_{}", agent_id), &mood],
    );
    mood
}

/// Get stored mood.
pub fn get_mood(conn: &Connection, agent_id: &str) -> String {
    conn.query_row(
        "SELECT value FROM config_store WHERE key = ?1",
        params![format!("mood_{}", agent_id)],
        |row| row.get(0),
    ).unwrap_or_else(|_| "neutral".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;
    use crate::memory::{Episode, episodic};

    #[test]
    fn test_mood_neutral_no_tasks() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        assert_eq!(calculate_mood(&conn, &agent.id), "neutral");
    }

    #[test]
    fn test_mood_thriving_after_successes() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        for i in 0..6 {
            episodic::insert_episode(&conn, &Episode {
                id: format!("ep{}", i), agent_id: agent.id.clone(),
                created_at: chrono::Utc::now().to_rfc3339(), event_type: "task_end".into(),
                summary: "done".into(), raw_data: None, task_id: None,
                outcome: Some("success".into()), tokens_used: 0, cost_cents: 0,
            }).expect("insert");
        }
        assert_eq!(calculate_mood(&conn, &agent.id), "thriving");
    }

    #[test]
    fn test_mood_prompt_varies() {
        assert!(!mood_prompt("thriving").is_empty());
        assert!(!mood_prompt("struggling").is_empty());
        assert!(mood_prompt("neutral").is_empty());
    }
}
