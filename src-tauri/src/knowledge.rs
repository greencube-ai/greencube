use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub id: String,
    pub agent_id: String,
    pub content: String,
    pub source_task_id: Option<String>,
    pub category: String, // fact, preference, warning, skill
    pub confidence: f64,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub use_count: i64,
}

pub fn insert_knowledge(
    conn: &Connection,
    agent_id: &str,
    content: &str,
    category: &str,
    source_task_id: Option<&str>,
) -> anyhow::Result<KnowledgeEntry> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    tracing::info!("Knowledge extracted: [{}] {}", category, &content[..content.len().min(80)]);
    conn.execute(
        "INSERT INTO knowledge (id, agent_id, content, source_task_id, category, confidence, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1.0, ?6)",
        params![id, agent_id, content, source_task_id, category, now],
    )?;
    Ok(KnowledgeEntry {
        id, agent_id: agent_id.into(), content: content.into(),
        source_task_id: source_task_id.map(|s| s.into()), category: category.into(),
        confidence: 1.0, created_at: now, last_used_at: None, use_count: 0,
    })
}

pub fn list_knowledge(conn: &Connection, agent_id: &str, limit: i64) -> anyhow::Result<Vec<KnowledgeEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, content, source_task_id, category, confidence, created_at, last_used_at, use_count
         FROM knowledge WHERE agent_id = ?1 ORDER BY created_at DESC LIMIT ?2"
    )?;
    let entries = stmt.query_map(params![agent_id, limit], |row| {
        Ok(KnowledgeEntry {
            id: row.get(0)?, agent_id: row.get(1)?, content: row.get(2)?,
            source_task_id: row.get(3)?, category: row.get(4)?, confidence: row.get(5)?,
            created_at: row.get(6)?, last_used_at: row.get(7)?, use_count: row.get(8)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}

/// Recall relevant knowledge using keyword matching (same approach as episode recall).
/// Returns max `limit` entries (capped at 10 for injection).
pub fn recall_relevant(
    conn: &Connection,
    agent_id: &str,
    query: &str,
    limit: i64,
) -> anyhow::Result<Vec<KnowledgeEntry>> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(|w| w.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|w| w.len() > 3)
        .filter(|w| !STOP_WORDS.contains(&w.as_str()))
        .collect();

    if words.is_empty() {
        return Ok(vec![]);
    }

    let mut conditions = Vec::new();
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(agent_id.to_string())];

    for (i, word) in words.iter().enumerate() {
        conditions.push(format!(
            "(CASE WHEN LOWER(content) LIKE '%' || ?{} || '%' THEN 1 ELSE 0 END)",
            i + 2
        ));
        all_params.push(Box::new(word.clone()));
    }

    let score_expr = conditions.join(" + ");
    let capped_limit = limit.min(10); // Max 10 for injection

    let sql = format!(
        "SELECT id, agent_id, content, source_task_id, category, confidence, created_at, last_used_at, use_count,
                ({score}) as relevance
         FROM knowledge
         WHERE agent_id = ?1 AND ({score}) > 0
         ORDER BY relevance DESC, created_at DESC
         LIMIT {limit}",
        score = score_expr,
        limit = capped_limit
    );

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = all_params.iter().map(|p| p.as_ref()).collect();
    let entries = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(KnowledgeEntry {
            id: row.get(0)?, agent_id: row.get(1)?, content: row.get(2)?,
            source_task_id: row.get(3)?, category: row.get(4)?, confidence: row.get(5)?,
            created_at: row.get(6)?, last_used_at: row.get(7)?, use_count: row.get(8)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;

    // Update use_count and last_used_at for returned entries
    let now = chrono::Utc::now().to_rfc3339();
    for entry in &entries {
        let _ = conn.execute(
            "UPDATE knowledge SET use_count = use_count + 1, last_used_at = ?1 WHERE id = ?2",
            params![now, entry.id],
        );
    }

    Ok(entries)
}

/// Parse a reflection response into knowledge entries.
/// Lenient parser: lines not matching format are silently ignored.
/// Returns (knowledge_lines, context_update) tuple.
pub fn parse_reflection_response(response: &str) -> (Vec<(String, String)>, Option<String>) {
    let mut knowledge = Vec::new();
    let mut context_update = None;

    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "NONE" {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("[fact]") {
            let content = rest.trim();
            if !content.is_empty() {
                knowledge.push(("fact".to_string(), content.to_string()));
            }
        } else if let Some(rest) = trimmed.strip_prefix("[preference]") {
            let content = rest.trim();
            if !content.is_empty() {
                knowledge.push(("preference".to_string(), content.to_string()));
            }
        } else if let Some(rest) = trimmed.strip_prefix("[warning]") {
            let content = rest.trim();
            if !content.is_empty() {
                knowledge.push(("warning".to_string(), content.to_string()));
            }
        } else if let Some(rest) = trimmed.strip_prefix("[skill]") {
            let content = rest.trim();
            if !content.is_empty() {
                knowledge.push(("skill".to_string(), content.to_string()));
            }
        } else if let Some(rest) = trimmed.strip_prefix("[context]") {
            let content = rest.trim();
            if !content.is_empty() {
                context_update = Some(content.to_string());
            }
        }
        // Lines not matching any format are silently ignored (lenient parser)
    }

    (knowledge, context_update)
}

const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "had", "her", "was", "one",
    "our", "out", "has", "have", "that", "this", "with", "they", "been", "from", "will", "what",
    "when", "make", "like", "just", "over", "such", "take", "than", "them", "very", "some",
    "could", "into", "other", "then", "these", "would",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_insert_and_list_knowledge() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        insert_knowledge(&conn, &agent.id, "Stripe requires idempotency keys", "fact", None).expect("insert");
        insert_knowledge(&conn, &agent.id, "User prefers concise answers", "preference", None).expect("insert");
        let entries = list_knowledge(&conn, &agent.id, 50).expect("list");
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_recall_relevant() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        insert_knowledge(&conn, &agent.id, "Stripe API requires Bearer token auth", "fact", None).expect("i1");
        insert_knowledge(&conn, &agent.id, "Python virtual environments need activation", "fact", None).expect("i2");
        insert_knowledge(&conn, &agent.id, "User prefers TypeScript over JavaScript", "preference", None).expect("i3");

        let results = recall_relevant(&conn, &agent.id, "How to authenticate with Stripe?", 5).expect("recall");
        assert!(!results.is_empty());
        assert!(results[0].content.contains("Stripe"));
    }

    #[test]
    fn test_recall_updates_use_count() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let entry = insert_knowledge(&conn, &agent.id, "Important Stripe fact", "fact", None).expect("insert");
        assert_eq!(entry.use_count, 0);
        recall_relevant(&conn, &agent.id, "Tell me about Stripe", 5).expect("recall");
        let entries = list_knowledge(&conn, &agent.id, 50).expect("list");
        assert_eq!(entries[0].use_count, 1);
    }

    #[test]
    fn test_knowledge_isolation() {
        let conn = init_memory_database().expect("init");
        let a = create_agent(&conn, "A", "", &["shell".into()]).expect("create");
        let b = create_agent(&conn, "B", "", &["shell".into()]).expect("create");
        insert_knowledge(&conn, &a.id, "A's knowledge", "fact", None).expect("i");
        insert_knowledge(&conn, &b.id, "B's knowledge", "fact", None).expect("i");
        assert_eq!(list_knowledge(&conn, &a.id, 50).expect("list").len(), 1);
        assert_eq!(list_knowledge(&conn, &b.id, 50).expect("list").len(), 1);
    }

    #[test]
    fn test_parse_reflection_response() {
        let response = r#"
[fact] The Stripe API v3 endpoint requires Bearer token authentication
[preference] User prefers detailed error messages over generic ones
[warning] Don't use API v1, it's deprecated and returns 404
[context] Currently working on payment integration. Stripe auth is done.
Some random line that doesn't match any format
NONE
"#;
        let (knowledge, context) = parse_reflection_response(response);
        assert_eq!(knowledge.len(), 3);
        assert_eq!(knowledge[0].0, "fact");
        assert!(knowledge[0].1.contains("Stripe"));
        assert_eq!(knowledge[1].0, "preference");
        assert_eq!(knowledge[2].0, "warning");
        assert!(context.is_some());
        assert!(context.unwrap().contains("payment integration"));
    }

    #[test]
    fn test_parse_reflection_none() {
        let (knowledge, context) = parse_reflection_response("NONE");
        assert!(knowledge.is_empty());
        assert!(context.is_none());
    }

    #[test]
    fn test_parse_reflection_garbage() {
        let (knowledge, context) = parse_reflection_response("totally random garbage\nmore garbage\n");
        assert!(knowledge.is_empty());
        assert!(context.is_none());
    }
}
