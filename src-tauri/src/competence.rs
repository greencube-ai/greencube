use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetenceEntry {
    pub domain: String,
    pub confidence: f64,
    pub task_count: i64,
    pub success_count: i64,
    pub trend: String,
    pub last_assessed: String,
}

/// Update competence for a domain after a task.
/// Called from reflection parsing when [domain] is extracted.
pub fn update_competence(
    conn: &Connection,
    agent_id: &str,
    domain: &str,
    success: bool,
    llm_confidence: Option<f64>,
) -> anyhow::Result<()> {
    let domain = domain.to_lowercase().trim().to_string();
    if domain.is_empty() { return Ok(()); }

    let now = chrono::Utc::now().to_rfc3339();

    // Try to get existing entry
    let existing = conn.query_row(
        "SELECT confidence, task_count, success_count FROM competence_map WHERE agent_id = ?1 AND domain = ?2",
        params![agent_id, domain],
        |row| Ok((row.get::<_, f64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
    );

    match existing {
        Ok((old_confidence, task_count, success_count)) => {
            let new_task_count = task_count + 1;
            let new_success_count = if success { success_count + 1 } else { success_count };
            let actual_rate = new_success_count as f64 / new_task_count as f64;

            // Weight actual rate (70%) with LLM self-assessment (30%)
            let new_confidence = if let Some(llm_conf) = llm_confidence {
                actual_rate * 0.7 + llm_conf * 0.3
            } else {
                actual_rate
            };

            let trend = if new_confidence > old_confidence + 0.05 {
                "improving"
            } else if new_confidence < old_confidence - 0.05 {
                "declining"
            } else {
                "stable"
            };

            conn.execute(
                "UPDATE competence_map SET confidence = ?1, task_count = ?2, success_count = ?3, trend = ?4, last_assessed = ?5 WHERE agent_id = ?6 AND domain = ?7",
                params![new_confidence, new_task_count, new_success_count, trend, now, agent_id, domain],
            )?;
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            let id = uuid::Uuid::new_v4().to_string();
            let confidence = if let Some(llm_conf) = llm_confidence {
                if success { 0.7 * 1.0 + 0.3 * llm_conf } else { 0.7 * 0.0 + 0.3 * llm_conf }
            } else {
                if success { 1.0 } else { 0.0 }
            };
            conn.execute(
                "INSERT INTO competence_map (id, agent_id, domain, confidence, task_count, success_count, last_assessed, trend)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, 'stable')",
                params![id, agent_id, domain, confidence, if success { 1i64 } else { 0i64 }, now],
            )?;
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

/// Adjust confidence from feedback (praise +0.05, correction -0.05).
pub fn adjust_confidence(conn: &Connection, agent_id: &str, domain: &str, delta: f64) -> anyhow::Result<()> {
    let domain = domain.to_lowercase();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE competence_map SET confidence = MIN(1.0, MAX(0.0, confidence + ?1)), last_assessed = ?2 WHERE agent_id = ?3 AND domain = ?4",
        params![delta, now, agent_id, domain],
    )?;
    Ok(())
}

pub fn get_competence_map(conn: &Connection, agent_id: &str) -> anyhow::Result<Vec<CompetenceEntry>> {
    let mut stmt = conn.prepare(
        "SELECT domain, confidence, task_count, success_count, trend, last_assessed
         FROM competence_map WHERE agent_id = ?1 ORDER BY task_count DESC"
    )?;
    let entries = stmt.query_map(params![agent_id], |row| {
        Ok(CompetenceEntry {
            domain: row.get(0)?, confidence: row.get(1)?, task_count: row.get(2)?,
            success_count: row.get(3)?, trend: row.get(4)?, last_assessed: row.get(5)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}

/// Get the most recently assessed domain for an agent.
pub fn get_most_recent_domain(conn: &Connection, agent_id: &str) -> anyhow::Result<Option<String>> {
    let result = conn.query_row(
        "SELECT domain FROM competence_map WHERE agent_id = ?1 ORDER BY last_assessed DESC LIMIT 1",
        params![agent_id],
        |row| row.get::<_, String>(0),
    );
    match result {
        Ok(domain) => Ok(Some(domain)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get domains where the agent has <50% confidence with 3+ tasks. These are honest limitations.
/// Tied to commandment 2 (never lie) and commandment 9 (flag uncertainty).
pub fn get_limitations(conn: &Connection, agent_id: &str) -> anyhow::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT domain, CAST(confidence * 100 AS INTEGER) FROM competence_map
         WHERE agent_id = ?1 AND confidence < 0.5 AND task_count >= 3
         ORDER BY confidence ASC"
    )?;
    let limitations = stmt.query_map(params![agent_id], |row| {
        let domain: String = row.get(0)?;
        let pct: i64 = row.get(1)?;
        Ok(format!("{} ({}% success rate)", domain, pct))
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(limitations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_first_competence_update() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        update_competence(&conn, &agent.id, "python", true, Some(0.8)).expect("update");
        let map = get_competence_map(&conn, &agent.id).expect("get");
        assert_eq!(map.len(), 1);
        assert_eq!(map[0].domain, "python");
        assert!(map[0].confidence > 0.5);
    }

    #[test]
    fn test_competence_trend() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        // Start high, then fail a lot
        update_competence(&conn, &agent.id, "css", true, Some(0.9)).expect("u1");
        update_competence(&conn, &agent.id, "css", false, Some(0.3)).expect("u2");
        update_competence(&conn, &agent.id, "css", false, Some(0.2)).expect("u3");
        let map = get_competence_map(&conn, &agent.id).expect("get");
        assert_eq!(map[0].trend, "declining");
    }

    #[test]
    fn test_limitations() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        // Create a domain with low confidence and enough tasks
        update_competence(&conn, &agent.id, "css", false, Some(0.2)).expect("u1");
        update_competence(&conn, &agent.id, "css", false, Some(0.2)).expect("u2");
        update_competence(&conn, &agent.id, "css", true, Some(0.3)).expect("u3");
        let limits = get_limitations(&conn, &agent.id).expect("get");
        assert!(!limits.is_empty());
        assert!(limits[0].contains("css"));
    }

    #[test]
    fn test_adjust_confidence() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        update_competence(&conn, &agent.id, "python", true, Some(0.7)).expect("initial");
        let before = get_competence_map(&conn, &agent.id).expect("get")[0].confidence;
        adjust_confidence(&conn, &agent.id, "python", 0.05).expect("adjust");
        let after = get_competence_map(&conn, &agent.id).expect("get")[0].confidence;
        assert!(after > before);
    }
}
