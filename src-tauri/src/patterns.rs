use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPattern {
    pub id: String,
    pub agent_id: String,
    pub pattern_description: String,
    pub frequency: i64,
    pub last_seen: String,
    pub preparation_note: Option<String>,
}

pub fn store_pattern(
    conn: &Connection,
    agent_id: &str,
    description: &str,
    preparation: Option<&str>,
) -> anyhow::Result<()> {
    // Check if similar pattern exists (exact match on description)
    let existing = conn.query_row(
        "SELECT id, frequency FROM task_patterns WHERE agent_id = ?1 AND pattern_description = ?2",
        params![agent_id, description],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    );

    match existing {
        Ok((id, freq)) => {
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE task_patterns SET frequency = ?1, last_seen = ?2, preparation_note = COALESCE(?3, preparation_note) WHERE id = ?4",
                params![freq + 1, now, preparation, id],
            )?;
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            let id = uuid::Uuid::new_v4().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO task_patterns (id, agent_id, pattern_description, frequency, last_seen, preparation_note, created_at)
                 VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6)",
                params![id, agent_id, description, now, preparation, now],
            )?;
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

/// Check if any patterns match the current user message.
/// Uses simple keyword matching against pattern descriptions.
pub fn check_patterns(conn: &Connection, agent_id: &str, current_message: &str) -> anyhow::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT pattern_description, preparation_note FROM task_patterns
         WHERE agent_id = ?1 AND preparation_note IS NOT NULL
         ORDER BY frequency DESC LIMIT 10"
    )?;
    let patterns: Vec<(String, String)> = stmt.query_map(params![agent_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?.collect::<Result<Vec<_>, _>>()?;

    let msg_lower = current_message.to_lowercase();
    let mut matches = Vec::new();

    for (desc, prep) in patterns {
        // Check if any significant words from the pattern appear in the current message
        let pattern_words: Vec<&str> = desc.split_whitespace()
            .filter(|w| w.len() > 3)
            .collect();
        let match_count = pattern_words.iter()
            .filter(|w| msg_lower.contains(&w.to_lowercase()))
            .count();
        if match_count > 0 && match_count >= pattern_words.len() / 2 {
            matches.push(prep);
        }
    }
    Ok(matches)
}

/// Parse [pattern] entries from idle thinking response.
/// Format: [pattern] description | preparation suggestion
pub fn parse_patterns(response: &str) -> Vec<(String, Option<String>)> {
    let mut patterns = Vec::new();
    for line in response.lines() {
        if let Some(text) = line.trim().strip_prefix("[pattern]") {
            let text = text.trim();
            if text.is_empty() { continue; }
            if let Some((desc, prep)) = text.split_once('|') {
                patterns.push((desc.trim().to_string(), Some(prep.trim().to_string())));
            } else {
                patterns.push((text.to_string(), None));
            }
        }
    }
    patterns
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_store_and_frequency() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        store_pattern(&conn, &agent.id, "Stripe tasks followed by testing", Some("Prepare test cases")).expect("s1");
        store_pattern(&conn, &agent.id, "Stripe tasks followed by testing", None).expect("s2");
        // Frequency should be 2 now
        let freq: i64 = conn.query_row(
            "SELECT frequency FROM task_patterns WHERE agent_id = ?1", params![agent.id], |row| row.get(0),
        ).expect("get");
        assert_eq!(freq, 2);
    }

    #[test]
    fn test_check_patterns_match() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        store_pattern(&conn, &agent.id, "Stripe payment integration", Some("Check API version first")).expect("store");
        let matches = check_patterns(&conn, &agent.id, "Help me with the Stripe payment flow").expect("check");
        assert!(!matches.is_empty());
        assert!(matches[0].contains("API version"));
    }

    #[test]
    fn test_parse_patterns() {
        let response = "[pattern] API work followed by testing | Prepare test harness\n[pattern] Morning debugging sessions\nother text";
        let patterns = parse_patterns(response);
        assert_eq!(patterns.len(), 2);
        assert_eq!(patterns[0].1.as_deref(), Some("Prepare test harness"));
        assert!(patterns[1].1.is_none());
    }
}
