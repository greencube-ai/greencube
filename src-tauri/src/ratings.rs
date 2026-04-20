use rusqlite::{params, Connection};

pub fn rate_response(conn: &Connection, agent_id: &str, task_id: &str, rating: i32) -> anyhow::Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO response_ratings (id, agent_id, task_id, rating, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, agent_id, task_id, rating, now],
    )?;
    Ok(())
}

/// Get approval rate as percentage (0.0-1.0) over last N rated tasks.
pub fn get_approval_rate(conn: &Connection, agent_id: &str, last_n: i64) -> anyhow::Result<f64> {
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM response_ratings WHERE agent_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        params![agent_id, last_n],
        |row| row.get(0),
    )?;
    if total == 0 { return Ok(0.0); }
    let positive: i64 = conn.query_row(
        "SELECT COUNT(*) FROM (SELECT rating FROM response_ratings WHERE agent_id = ?1 ORDER BY created_at DESC LIMIT ?2) WHERE rating = 1",
        params![agent_id, last_n],
        |row| row.get(0),
    )?;
    Ok(positive as f64 / total as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_rate_and_get_approval() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        rate_response(&conn, &agent.id, "task1", 1).expect("r1");
        rate_response(&conn, &agent.id, "task2", 1).expect("r2");
        rate_response(&conn, &agent.id, "task3", -1).expect("r3");
        let rate = get_approval_rate(&conn, &agent.id, 30).expect("get");
        assert!((rate - 0.666).abs() < 0.01);
    }

}
