use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub agent_id: String,
    pub capability: String,
    pub confidence: f64,
    pub source: String, // auto, manual
    pub created_at: String,
}

pub fn list_capabilities(conn: &Connection, agent_id: &str) -> anyhow::Result<Vec<Capability>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, capability, confidence, source, created_at FROM agent_capabilities WHERE agent_id = ?1 ORDER BY confidence DESC"
    )?;
    let caps = stmt.query_map(params![agent_id], |row| {
        Ok(Capability {
            id: row.get(0)?, agent_id: row.get(1)?, capability: row.get(2)?,
            confidence: row.get(3)?, source: row.get(4)?, created_at: row.get(5)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(caps)
}

/// Search agents by capability (case-insensitive LIKE).
pub fn search_capabilities(conn: &Connection, query: &str) -> anyhow::Result<Vec<(String, String, String, f64)>> {
    let pattern = format!("%{}%", query.to_lowercase());
    let mut stmt = conn.prepare(
        "SELECT ac.agent_id, a.name, ac.capability, ac.confidence
         FROM agent_capabilities ac
         JOIN agents a ON a.id = ac.agent_id
         WHERE LOWER(ac.capability) LIKE ?1
         ORDER BY ac.confidence DESC LIMIT 20"
    )?;
    let results = stmt.query_map(params![pattern], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, f64>(3)?))
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(results)
}
