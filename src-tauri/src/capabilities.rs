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

pub fn add_capability(conn: &Connection, agent_id: &str, capability: &str, confidence: f64, source: &str) -> anyhow::Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO agent_capabilities (id, agent_id, capability, confidence, source, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, agent_id, capability, confidence, source, now],
    )?;
    Ok(())
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

/// Clear auto-generated capabilities for an agent (before regeneration).
pub fn clear_auto_capabilities(conn: &Connection, agent_id: &str) -> anyhow::Result<()> {
    conn.execute(
        "DELETE FROM agent_capabilities WHERE agent_id = ?1 AND source = 'auto'",
        params![agent_id],
    )?;
    Ok(())
}

/// Parse capabilities from profile generation response.
pub fn parse_capabilities(response: &str) -> Vec<String> {
    response.lines()
        .filter_map(|line| {
            line.trim().strip_prefix("[capability]").map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_add_and_list_capabilities() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        add_capability(&conn, &agent.id, "code review", 0.85, "auto").expect("add1");
        add_capability(&conn, &agent.id, "python debugging", 0.7, "auto").expect("add2");
        let caps = list_capabilities(&conn, &agent.id).expect("list");
        assert_eq!(caps.len(), 2);
        assert_eq!(caps[0].capability, "code review"); // Higher confidence first
    }

    #[test]
    fn test_search_capabilities() {
        let conn = init_memory_database().expect("init");
        let a = create_agent(&conn, "CodeBot", "", &["shell".into()]).expect("create");
        let b = create_agent(&conn, "DataBot", "", &["shell".into()]).expect("create");
        add_capability(&conn, &a.id, "code review", 0.9, "auto").expect("add1");
        add_capability(&conn, &b.id, "data analysis", 0.8, "auto").expect("add2");
        add_capability(&conn, &a.id, "code debugging", 0.7, "auto").expect("add3");

        let results = search_capabilities(&conn, "code").expect("search");
        assert_eq!(results.len(), 2); // code review + code debugging
        assert!(results.iter().all(|(_, name, _, _)| name == "CodeBot"));
    }

    #[test]
    fn test_parse_capabilities() {
        let response = "[capability] code review\n[capability] Python debugging\nsome random text\n";
        let caps = parse_capabilities(response);
        assert_eq!(caps.len(), 2);
        assert_eq!(caps[0], "code review");
    }

    #[test]
    fn test_clear_auto_capabilities() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        add_capability(&conn, &agent.id, "auto cap", 0.5, "auto").expect("add1");
        add_capability(&conn, &agent.id, "manual cap", 0.5, "manual").expect("add2");
        clear_auto_capabilities(&conn, &agent.id).expect("clear");
        let caps = list_capabilities(&conn, &agent.id).expect("list");
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].source, "manual");
    }
}
