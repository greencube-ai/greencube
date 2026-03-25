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

pub fn store_feedback(
    conn: &Connection,
    agent_id: &str,
    signal_type: &str,
    content: &str,
    task_id: Option<&str>,
) -> anyhow::Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO feedback_signals (id, agent_id, signal_type, content, source_task_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, agent_id, signal_type, content, task_id, now],
    )?;

    // Also store as knowledge entry for recall
    let category = if signal_type == "praise" { "preference" } else { "warning" };
    let knowledge_content = if signal_type == "praise" {
        format!("User likes when I {}", content)
    } else {
        format!("User corrected: {}. Do differently next time.", content)
    };
    let _ = crate::knowledge::insert_knowledge(conn, agent_id, &knowledge_content, category, task_id);

    Ok(())
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

/// Parse feedback from reflection response lines.
pub fn parse_feedback(response: &str) -> Vec<(String, String)> {
    let mut feedback = Vec::new();
    for line in response.lines() {
        let trimmed = line.trim();
        if let Some(text) = trimmed.strip_prefix("[praise]") {
            let text = text.trim();
            if !text.is_empty() { feedback.push(("praise".into(), text.into())); }
        } else if let Some(text) = trimmed.strip_prefix("[correction]") {
            let text = text.trim();
            if !text.is_empty() { feedback.push(("correction".into(), text.into())); }
        }
    }
    feedback
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_store_and_get_feedback() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        store_feedback(&conn, &agent.id, "praise", "Good error handling", None).expect("store");
        store_feedback(&conn, &agent.id, "correction", "Too verbose", None).expect("store");
        let signals = get_recent_feedback(&conn, &agent.id, 10).expect("get");
        assert_eq!(signals.len(), 2);
    }

    #[test]
    fn test_parse_feedback() {
        let response = "[praise] excellent edge case coverage\n[correction] response was too long\nrandom line";
        let feedback = parse_feedback(response);
        assert_eq!(feedback.len(), 2);
        assert_eq!(feedback[0].0, "praise");
        assert_eq!(feedback[1].0, "correction");
    }

    #[test]
    fn test_feedback_creates_knowledge() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        store_feedback(&conn, &agent.id, "correction", "Be more concise", None).expect("store");
        let knowledge = crate::knowledge::list_knowledge(&conn, &agent.id, 10).expect("list");
        assert!(knowledge.iter().any(|k| k.content.contains("concise")));
    }
}
