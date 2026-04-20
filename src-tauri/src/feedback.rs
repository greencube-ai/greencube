use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackSignal {
    pub id: String,
    pub agent_id: String,
    pub signal_type: String, // praise, correction
    pub content: String,
    pub source_task_id: Option<String>,
    pub created_at: String,
}

pub fn get_recent_feedback(conn: &Connection, agent_id: &str, limit: i64) -> anyhow::Result<Vec<FeedbackSignal>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, signal_type, content, source_task_id, created_at
         FROM feedback_signals WHERE agent_id = ?1 ORDER BY created_at DESC LIMIT ?2"
    )?;
    let signals = stmt.query_map(params![agent_id, limit], |row| {
        Ok(FeedbackSignal {
            id: row.get(0)?, agent_id: row.get(1)?, signal_type: row.get(2)?,
            content: row.get(3)?, source_task_id: row.get(4)?, created_at: row.get(5)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(signals)
}
