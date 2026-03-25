use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSnapshot {
    pub date: String,
    pub total_tasks: i64,
    pub successful_tasks: i64,
    pub knowledge_count: i64,
    pub total_spend_cents: i64,
}

/// UPSERT today's metrics for an agent. Called from finish_task.
pub fn record_metric(conn: &Connection, agent_id: &str) -> anyhow::Result<()> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    // Get current agent stats
    let (total_tasks, successful_tasks, total_spend): (i64, i64, i64) = conn.query_row(
        "SELECT total_tasks, successful_tasks, total_spend_cents FROM agents WHERE id = ?1",
        params![agent_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    // Count knowledge entries
    let knowledge_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM knowledge WHERE agent_id = ?1",
        params![agent_id],
        |row| row.get(0),
    )?;

    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO agent_metrics (id, agent_id, date, total_tasks, successful_tasks, knowledge_count, total_spend_cents)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(agent_id, date) DO UPDATE SET
            total_tasks = ?4, successful_tasks = ?5, knowledge_count = ?6, total_spend_cents = ?7",
        params![id, agent_id, today, total_tasks, successful_tasks, knowledge_count, total_spend],
    )?;
    Ok(())
}

/// Get metric history for an agent (for charts).
pub fn get_metrics(conn: &Connection, agent_id: &str, days: i64) -> anyhow::Result<Vec<MetricSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT date, total_tasks, successful_tasks, knowledge_count, total_spend_cents
         FROM agent_metrics WHERE agent_id = ?1 ORDER BY date DESC LIMIT ?2"
    )?;
    let metrics = stmt.query_map(params![agent_id, days], |row| {
        Ok(MetricSnapshot {
            date: row.get(0)?,
            total_tasks: row.get(1)?,
            successful_tasks: row.get(2)?,
            knowledge_count: row.get(3)?,
            total_spend_cents: row.get(4)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_record_and_get_metrics() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        // Increment task counts first
        crate::identity::registry::increment_task_counts(&conn, &agent.id, true, 10).expect("inc");
        record_metric(&conn, &agent.id).expect("record");
        let metrics = get_metrics(&conn, &agent.id, 30).expect("get");
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].total_tasks, 1);
        assert_eq!(metrics[0].successful_tasks, 1);
    }

    #[test]
    fn test_upsert_same_day() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        crate::identity::registry::increment_task_counts(&conn, &agent.id, true, 10).expect("inc1");
        record_metric(&conn, &agent.id).expect("r1");
        crate::identity::registry::increment_task_counts(&conn, &agent.id, true, 10).expect("inc2");
        record_metric(&conn, &agent.id).expect("r2");
        let metrics = get_metrics(&conn, &agent.id, 30).expect("get");
        assert_eq!(metrics.len(), 1); // Still one entry for today
        assert_eq!(metrics[0].total_tasks, 2); // Updated value
    }
}
