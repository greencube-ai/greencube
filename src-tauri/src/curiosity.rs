use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

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
