use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummary {
    pub task_tokens: i64,
    pub background_tokens: i64,
    pub breakdown: Vec<(String, i64)>,
}

/// Record token usage for a specific category.
pub fn record_usage(conn: &Connection, agent_id: &str, category: &str, tokens: i64) -> anyhow::Result<()> {
    if tokens <= 0 { return Ok(()); }
    let id = uuid::Uuid::new_v4().to_string();
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO token_usage (id, agent_id, category, tokens, date, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, agent_id, category, tokens, today, now],
    )?;
    Ok(())
}

/// Get today's background token usage for an agent (everything except "task").
pub fn get_background_usage_today(conn: &Connection, agent_id: &str) -> anyhow::Result<i64> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(tokens), 0) FROM token_usage WHERE agent_id = ?1 AND date = ?2 AND category != 'task'",
        params![agent_id, today],
        |row| row.get(0),
    )?;
    Ok(total)
}

/// Check if there's enough budget remaining for an estimated call.
/// Returns true if we can proceed, false if budget would be exceeded.
pub fn has_budget_remaining(conn: &Connection, agent_id: &str, estimated_tokens: i64, budget: u64) -> anyhow::Result<bool> {
    let used = get_background_usage_today(conn, agent_id)?;
    Ok((used + estimated_tokens) <= budget as i64)
}

/// Get full usage summary for today (for dashboard).
pub fn get_usage_today(conn: &Connection, agent_id: &str) -> anyhow::Result<UsageSummary> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare(
        "SELECT category, COALESCE(SUM(tokens), 0) FROM token_usage WHERE agent_id = ?1 AND date = ?2 GROUP BY category"
    )?;
    let breakdown: Vec<(String, i64)> = stmt.query_map(params![agent_id, today], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?.collect::<Result<Vec<_>, _>>()?;

    let task_tokens = breakdown.iter().filter(|(c, _)| c == "task").map(|(_, t)| *t).sum();
    let background_tokens = breakdown.iter().filter(|(c, _)| c != "task").map(|(_, t)| *t).sum();

    Ok(UsageSummary { task_tokens, background_tokens, breakdown })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_record_and_get_usage() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        record_usage(&conn, &agent.id, "task", 500).expect("r1");
        record_usage(&conn, &agent.id, "reflection", 200).expect("r2");
        record_usage(&conn, &agent.id, "idle", 100).expect("r3");
        let summary = get_usage_today(&conn, &agent.id).expect("get");
        assert_eq!(summary.task_tokens, 500);
        assert_eq!(summary.background_tokens, 300);
    }

    #[test]
    fn test_budget_check() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        record_usage(&conn, &agent.id, "reflection", 9500).expect("record");
        // Budget is 10000. 9500 used. Estimated 300 = 9800. Should allow.
        assert!(has_budget_remaining(&conn, &agent.id, 300, 10000).expect("check"));
        // Estimated 600 = 10100. Should deny.
        assert!(!has_budget_remaining(&conn, &agent.id, 600, 10000).expect("check"));
    }
}
