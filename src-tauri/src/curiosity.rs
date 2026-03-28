use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

const MAX_CURIOSITIES: i64 = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Curiosity {
    pub id: String,
    pub agent_id: String,
    pub topic: String,
    pub source_task_id: Option<String>,
    pub priority: i64,
    pub explored: bool,
    pub created_at: String,
}

/// Add a curiosity. If the topic already exists, bump its priority instead.
pub fn add_curiosity(conn: &Connection, agent_id: &str, topic: &str, source_task_id: Option<&str>) -> anyhow::Result<()> {
    let topic = topic.trim().to_string();
    if topic.is_empty() { return Ok(()); }

    // Check if similar topic exists (case-insensitive partial match)
    let existing: Option<String> = conn.query_row(
        "SELECT id FROM curiosities WHERE agent_id = ?1 AND LOWER(topic) LIKE '%' || LOWER(?2) || '%' AND explored = 0",
        params![agent_id, &topic[..topic.len().min(20)]],
        |row| row.get(0),
    ).ok();

    if let Some(id) = existing {
        // Bump priority
        conn.execute("UPDATE curiosities SET priority = priority + 1 WHERE id = ?1", params![id])?;
        tracing::info!("Curiosity bumped: {} (agent {})", topic, agent_id);
    } else {
        // Enforce max limit — drop oldest explored ones first
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM curiosities WHERE agent_id = ?1",
            params![agent_id], |row| row.get(0),
        ).unwrap_or(0);

        if count >= MAX_CURIOSITIES {
            conn.execute(
                "DELETE FROM curiosities WHERE id = (SELECT id FROM curiosities WHERE agent_id = ?1 AND explored = 1 ORDER BY created_at ASC LIMIT 1)",
                params![agent_id],
            )?;
        }

        conn.execute(
            "INSERT INTO curiosities (id, agent_id, topic, source_task_id, priority, explored, created_at)
             VALUES (?1, ?2, ?3, ?4, 1, 0, ?5)",
            params![uuid::Uuid::new_v4().to_string(), agent_id, topic, source_task_id, chrono::Utc::now().to_rfc3339()],
        )?;
        tracing::info!("New curiosity: '{}' (agent {})", topic, agent_id);
    }
    Ok(())
}

/// Get the top unexplored curiosity (highest priority).
pub fn get_top_curiosity(conn: &Connection, agent_id: &str) -> anyhow::Result<Option<Curiosity>> {
    let result = conn.query_row(
        "SELECT id, agent_id, topic, source_task_id, priority, explored, created_at
         FROM curiosities WHERE agent_id = ?1 AND explored = 0
         ORDER BY priority DESC, created_at ASC LIMIT 1",
        params![agent_id],
        |row| Ok(Curiosity {
            id: row.get(0)?, agent_id: row.get(1)?, topic: row.get(2)?,
            source_task_id: row.get(3)?, priority: row.get(4)?,
            explored: row.get::<_, i32>(5)? != 0, created_at: row.get(6)?,
        }),
    );
    match result {
        Ok(c) => Ok(Some(c)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Mark a curiosity as explored.
pub fn mark_explored(conn: &Connection, id: &str) -> anyhow::Result<()> {
    conn.execute("UPDATE curiosities SET explored = 1 WHERE id = ?1", params![id])?;
    Ok(())
}

/// Get all curiosities for display.
pub fn list_curiosities(conn: &Connection, agent_id: &str) -> anyhow::Result<Vec<Curiosity>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, topic, source_task_id, priority, explored, created_at
         FROM curiosities WHERE agent_id = ?1 ORDER BY explored ASC, priority DESC LIMIT 20"
    )?;
    let rows = stmt.query_map(params![agent_id], |row| {
        Ok(Curiosity {
            id: row.get(0)?, agent_id: row.get(1)?, topic: row.get(2)?,
            source_task_id: row.get(3)?, priority: row.get(4)?,
            explored: row.get::<_, i32>(5)? != 0, created_at: row.get(6)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_add_and_get_curiosity() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        add_curiosity(&conn, &agent.id, "how does async work in Rust?", None).expect("add");
        let top = get_top_curiosity(&conn, &agent.id).expect("get");
        assert!(top.is_some());
        assert!(top.unwrap().topic.contains("async"));
    }

    #[test]
    fn test_priority_bumps_on_duplicate() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        add_curiosity(&conn, &agent.id, "how does async work?", None).expect("add1");
        add_curiosity(&conn, &agent.id, "how does async work?", None).expect("add2");
        let top = get_top_curiosity(&conn, &agent.id).expect("get").unwrap();
        assert_eq!(top.priority, 2);
    }

    #[test]
    fn test_mark_explored() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        add_curiosity(&conn, &agent.id, "test topic", None).expect("add");
        let c = get_top_curiosity(&conn, &agent.id).expect("get").unwrap();
        mark_explored(&conn, &c.id).expect("mark");
        let top = get_top_curiosity(&conn, &agent.id).expect("get");
        assert!(top.is_none()); // explored ones don't show as top
    }
}
