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

/// Get a cluster's task count.
pub fn get_cluster_depth(conn: &Connection, agent_id: &str, domain: &str) -> i64 {
    conn.query_row(
        "SELECT task_count FROM context_clusters WHERE agent_id = ?1 AND cluster_name = ?2",
        params![agent_id, domain],
        |row| row.get(0),
    ).unwrap_or(0)
}

/// Get all knowledge entries for a domain cluster (full "place memory").
pub fn recall_cluster_knowledge(conn: &Connection, agent_id: &str, domain: &str, limit: i64) -> Vec<crate::knowledge::KnowledgeEntry> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, content, source_task_id, category, confidence, created_at, last_used_at, use_count, COALESCE(valence, 0), COALESCE(success_when_used, 0), COALESCE(stale, 0)
         FROM knowledge WHERE agent_id = ?1 AND LOWER(content) LIKE '%' || LOWER(?2) || '%'
         ORDER BY created_at DESC LIMIT ?3"
    ).ok();

    stmt.as_mut().map(|s| {
        s.query_map(params![agent_id, domain, limit], |row| {
            Ok(crate::knowledge::KnowledgeEntry {
                id: row.get(0)?, agent_id: row.get(1)?, content: row.get(2)?,
                source_task_id: row.get(3)?, category: row.get(4)?, confidence: row.get(5)?,
                created_at: row.get(6)?, last_used_at: row.get(7)?, use_count: row.get(8)?,
                valence: row.get(9)?, success_when_used: row.get::<_, i64>(10).unwrap_or(0),
                stale: row.get::<_, i64>(11).unwrap_or(0) != 0,
            })
        }).ok()
            .map(|r| r.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_update_and_get_cluster() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        update_cluster(&conn, &agent.id, "python").expect("update");
        assert_eq!(get_cluster_depth(&conn, &agent.id, "python"), 1);
        update_cluster(&conn, &agent.id, "python").expect("update2");
        assert_eq!(get_cluster_depth(&conn, &agent.id, "python"), 2);
    }

    #[test]
    fn test_cluster_recall() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        crate::knowledge::insert_knowledge(&conn, &agent.id, "python async is fast", "fact", None).expect("k");
        let results = recall_cluster_knowledge(&conn, &agent.id, "python", 10);
        assert_eq!(results.len(), 1);
    }
}
