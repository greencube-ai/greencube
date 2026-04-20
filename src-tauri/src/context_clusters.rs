use rusqlite::{params, Connection};

/// Update or create a context cluster for a domain.
pub fn update_cluster(conn: &Connection, agent_id: &str, domain: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let updated = conn.execute(
        "UPDATE context_clusters SET task_count = task_count + 1, last_seen_at = ?1 WHERE agent_id = ?2 AND cluster_name = ?3",
        params![now, agent_id, domain],
    )?;
    if updated == 0 {
        conn.execute(
            "INSERT INTO context_clusters (id, agent_id, cluster_name, keywords, task_count, last_seen_at) VALUES (?1, ?2, ?3, ?4, 1, ?5)",
            params![uuid::Uuid::new_v4().to_string(), agent_id, domain, domain, now],
        )?;
    }
    Ok(())
}
