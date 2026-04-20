use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub id: String,
    pub agent_id: String,
    pub content: String,
    pub source_task_id: Option<String>,
    pub category: String, // fact, preference, warning, skill, synthesis, correction, praise
    pub confidence: f64,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub use_count: i64,
    pub valence: i32, // -2 to +2: emotional memory (-2=very frustrating, 0=neutral, +2=excellent)
    pub success_when_used: i64, // bumped when task after injection gets self-verify "good"
    pub stale: bool, // true = relevance score < 0.1, still stored but not injected
}

pub fn insert_knowledge(
    conn: &Connection,
    agent_id: &str,
    content: &str,
    category: &str,
    source_task_id: Option<&str>,
) -> anyhow::Result<KnowledgeEntry> {
    insert_knowledge_with_valence(conn, agent_id, content, category, source_task_id, 0)
}

pub fn insert_knowledge_with_valence(
    conn: &Connection,
    agent_id: &str,
    content: &str,
    category: &str,
    source_task_id: Option<&str>,
    valence: i32,
) -> anyhow::Result<KnowledgeEntry> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let valence = valence.clamp(-2, 2);
    tracing::info!("Knowledge extracted: [{}] (v={}) {}", category, valence, &content[..content.len().min(80)]);
    conn.execute(
        "INSERT INTO knowledge (id, agent_id, content, source_task_id, category, confidence, created_at, valence)
         VALUES (?1, ?2, ?3, ?4, ?5, 1.0, ?6, ?7)",
        params![id, agent_id, content, source_task_id, category, now, valence],
    )?;
    Ok(KnowledgeEntry {
        id, agent_id: agent_id.into(), content: content.into(),
        source_task_id: source_task_id.map(|s| s.into()), category: category.into(),
        confidence: 1.0, created_at: now, last_used_at: None, use_count: 0, valence,
        success_when_used: 0, stale: false,
    })
}

pub fn list_knowledge(conn: &Connection, agent_id: &str, limit: i64) -> anyhow::Result<Vec<KnowledgeEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, content, source_task_id, category, confidence, created_at, last_used_at, use_count, COALESCE(valence, 0), COALESCE(success_when_used, 0), COALESCE(stale, 0)
         FROM knowledge WHERE agent_id = ?1 ORDER BY created_at DESC LIMIT ?2"
    )?;
    let entries = stmt.query_map(params![agent_id, limit], |row| {
        Ok(knowledge_from_row(row, 0)?)
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}

fn knowledge_from_row(row: &rusqlite::Row, offset: usize) -> rusqlite::Result<KnowledgeEntry> {
    Ok(KnowledgeEntry {
        id: row.get(offset)?, agent_id: row.get(offset + 1)?, content: row.get(offset + 2)?,
        source_task_id: row.get(offset + 3)?, category: row.get(offset + 4)?, confidence: row.get(offset + 5)?,
        created_at: row.get(offset + 6)?, last_used_at: row.get(offset + 7)?, use_count: row.get(offset + 8)?,
        valence: row.get(offset + 9)?,
        success_when_used: row.get(offset + 10).unwrap_or(0),
        stale: row.get::<_, i64>(offset + 11).unwrap_or(0) != 0,
    })
}

/// Recall relevant knowledge using keyword matching + relevance scoring.
/// Filters out stale entries (score < 0.2). Returns max `limit` entries (capped at 10).
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

    // Fetch more than needed so we can re-rank by relevance score
    let sql = format!(
        "SELECT id, agent_id, content, source_task_id, category, confidence, created_at, last_used_at, use_count, COALESCE(valence, 0), COALESCE(success_when_used, 0), COALESCE(stale, 0)
         FROM knowledge
         WHERE agent_id = ?1 AND ({score}) > 0 AND COALESCE(stale, 0) = 0
         ORDER BY ({score}) DESC, created_at DESC
         LIMIT 30",
        score = score_expr,
    );

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = all_params.iter().map(|p| p.as_ref()).collect();
    let entries = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(knowledge_from_row(row, 0)?)
    })?.collect::<Result<Vec<_>, _>>()?;

    // Score and rank by memory decay relevance
    let now = chrono::Utc::now();
    let mut scored: Vec<(f64, KnowledgeEntry)> = entries.into_iter().map(|e| {
        let score = compute_relevance_score(&e, &now);
        (score, e)
    }).filter(|(score, _)| *score >= 0.2).collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let capped_limit = limit.min(10) as usize;
    let results: Vec<KnowledgeEntry> = scored.into_iter().take(capped_limit).map(|(_, e)| e).collect();

    // Update use_count and last_used_at for returned entries
    let now_str = now.to_rfc3339();
    for entry in &results {
        let _ = conn.execute(
            "UPDATE knowledge SET use_count = use_count + 1, last_used_at = ?1 WHERE id = ?2",
            params![now_str, entry.id],
        );
    }

    Ok(results)
}

/// Memory decay relevance score.
/// Higher = more valuable knowledge. Gold knowledge (high success, rarely used) scores well.
/// Noise (frequently stored, never helpful) scores low.
/// New entries (use_count == 0) always pass — they haven't had a chance to prove themselves.
fn compute_relevance_score(entry: &KnowledgeEntry, now: &chrono::DateTime<chrono::Utc>) -> f64 {
    // New entries always get injected until they've been used at least once
    if entry.use_count == 0 {
        return 1.0;
    }

    // Recency: 1.0 if used today, decays 0.05 per day unused
    let recency = if let Some(ref last_used) = entry.last_used_at {
        if let Ok(used_at) = chrono::DateTime::parse_from_rfc3339(last_used) {
            let days_since = now.signed_duration_since(used_at).num_days().max(0) as f64;
            (1.0 - days_since * 0.05).max(0.0)
        } else {
            0.3
        }
    } else {
        0.3
    };

    let use_score = (entry.use_count as f64).min(10.0) / 10.0; // normalize 0-1, cap at 10
    let success_score = (entry.success_when_used as f64).min(10.0) / 10.0; // normalize 0-1, cap at 10

    (use_score * 0.3) + (success_score * 0.5) + (recency * 0.2)
}

/// Mark entries with relevance score < 0.1 as stale. Never deleted, just not injected.
pub fn mark_stale_entries(conn: &Connection, agent_id: &str) -> anyhow::Result<i64> {
    let all = list_knowledge(conn, agent_id, 500)?;
    let now = chrono::Utc::now();
    let mut marked = 0i64;
    for entry in &all {
        let score = compute_relevance_score(entry, &now);
        if score < 0.1 && !entry.stale {
            conn.execute("UPDATE knowledge SET stale = 1 WHERE id = ?1", params![entry.id])?;
            marked += 1;
        } else if score >= 0.2 && entry.stale {
            // Revive: if score recovered (got used again), unmark stale
            conn.execute("UPDATE knowledge SET stale = 0 WHERE id = ?1", params![entry.id])?;
        }
    }
    if marked > 0 {
        tracing::info!("Memory decay: marked {} entries as stale for agent {}", marked, agent_id);
    }
    Ok(marked)
}

/// Cross-agent learning: find knowledge from OTHER agents that's relevant to a query.
/// Excludes the requesting agent. Returns entries from habitat neighbors.
pub fn recall_habitat_knowledge(
    conn: &Connection,
    excluded_agent_id: &str,
    query: &str,
    limit: i64,
) -> anyhow::Result<Vec<(String, KnowledgeEntry)>> {
    // agent_name, entry pairs
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
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(excluded_agent_id.to_string())];

    for (i, word) in words.iter().enumerate() {
        conditions.push(format!(
            "(CASE WHEN LOWER(k.content) LIKE '%' || ?{} || '%' THEN 1 ELSE 0 END)",
            i + 2
        ));
        all_params.push(Box::new(word.clone()));
    }

    let score_expr = conditions.join(" + ");
    let sql = format!(
        "SELECT a.name, k.id, k.agent_id, k.content, k.source_task_id, k.category, k.confidence, k.created_at, k.last_used_at, k.use_count, COALESCE(k.valence, 0), COALESCE(k.success_when_used, 0), COALESCE(k.stale, 0)
         FROM knowledge k JOIN agents a ON a.id = k.agent_id
         WHERE k.agent_id != ?1 AND ({}) > 0 AND COALESCE(k.stale, 0) = 0
         ORDER BY ({}) DESC
         LIMIT {}",
        score_expr, score_expr, limit
    );

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = all_params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok((
            row.get::<_, String>(0)?,
            knowledge_from_row(row, 1)?,
        ))
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Parse a reflection response into knowledge entries.
/// Finds [tag] or [tag valence=N] ANYWHERE in the text.
/// Returns (knowledge_lines_with_valence, context_update, domain) tuple.
pub fn parse_reflection_response(response: &str) -> (Vec<(String, String, i32)>, Option<String>, Option<String>) {
    let mut knowledge: Vec<(String, String, i32)> = Vec::new(); // (category, content, valence)
    let mut context_update = None;
    let mut domain = None;

    if response.trim() == "NONE" || response.trim().is_empty() {
        return (knowledge, context_update, domain);
    }

    // Match tags with optional valence: [fact], [fact valence=-1], [warning valence=2], etc.
    let base_tags = ["fact", "preference", "warning", "skill", "curious", "context", "domain"];

    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        for base_tag in &base_tags {
            // Find [tag] or [tag valence=N]
            let simple_tag = format!("[{}]", base_tag);
            let valence_prefix = format!("[{} valence=", base_tag);

            let (found_pos, tag_end, valence) = if let Some(pos) = trimmed.find(&valence_prefix) {
                // Parse valence number
                let after_eq = pos + valence_prefix.len();
                let close = trimmed[after_eq..].find(']').map(|p| after_eq + p);
                if let Some(close_pos) = close {
                    let val_str = &trimmed[after_eq..close_pos];
                    let val: i32 = val_str.parse().unwrap_or(0).clamp(-2, 2);
                    (Some(pos), close_pos + 1, val)
                } else {
                    continue;
                }
            } else if let Some(pos) = trimmed.find(&simple_tag) {
                (Some(pos), pos + simple_tag.len(), 0)
            } else {
                continue;
            };

            if let Some(_pos) = found_pos {
                let content = trimmed[tag_end..].trim();
                if content.is_empty() { continue; }

                match *base_tag {
                    "context" => { context_update = Some(content.to_string()); }
                    "domain" => { domain = Some(content.split_whitespace().next().unwrap_or(content).to_lowercase()); }
                    _ => { knowledge.push((base_tag.to_string(), content.to_string(), valence)); }
                }
                break; // One tag per line
            }
        }
    }

    (knowledge, context_update, domain)
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
        let (knowledge, context, domain) = parse_reflection_response(response);
        assert_eq!(knowledge.len(), 3);
        assert_eq!(knowledge[0].0, "fact");
        assert!(knowledge[0].1.contains("Stripe"));
        assert_eq!(knowledge[1].0, "preference");
        assert_eq!(knowledge[2].0, "warning");
        assert!(context.is_some());
        assert!(context.unwrap().contains("payment integration"));
        assert!(domain.is_none()); // no [domain] tag in this test
    }

    #[test]
    fn test_parse_reflection_none() {
        let (knowledge, context, domain) = parse_reflection_response("NONE");
        assert!(knowledge.is_empty());
        assert!(context.is_none());
        assert!(domain.is_none());
    }

    #[test]
    fn test_parse_reflection_garbage() {
        let (knowledge, context, domain) = parse_reflection_response("totally random garbage\nmore garbage\n");
        assert!(knowledge.is_empty());
        assert!(context.is_none());
        assert!(domain.is_none());
    }

    #[test]
    fn test_parse_reflection_midline_tags() {
        let response = r#"1. What key facts did you learn? [fact] The user is interested in prime-checking functions
2. What should you remember? [preference] The user likes mathematical programming examples
3. Were there mistakes? [warning] No significant issues encountered
4. Update context: [context] User exploring math algorithms"#;
        let (knowledge, context, _domain) = parse_reflection_response(response);
        assert_eq!(knowledge.len(), 3, "Expected 3 knowledge entries, got {}: {:?}", knowledge.len(), knowledge);
        assert_eq!(knowledge[0].0, "fact");
        assert!(knowledge[0].1.contains("prime"), "fact should contain 'prime': {}", knowledge[0].1);
        assert_eq!(knowledge[1].0, "preference");
        assert_eq!(knowledge[2].0, "warning");
        assert!(context.is_some());
        assert!(context.unwrap().contains("math"), "context should contain 'math'");
    }

    #[test]
    fn test_parse_reflection_with_valence() {
        let response = "[fact valence=-1] The Stripe webhook API was difficult\n[warning valence=-2] Rate limiting caused multiple failures";
        let (knowledge, _, _) = parse_reflection_response(response);
        assert_eq!(knowledge.len(), 2);
        assert_eq!(knowledge[0].2, -1); // valence
        assert_eq!(knowledge[1].2, -2); // valence
    }

    #[test]
    fn test_parse_reflection_with_domain() {
        let response = "[fact] User asked about database indexing\n[domain] database\n[context] Working on query optimization";
        let (knowledge, context, domain) = parse_reflection_response(response);
        assert_eq!(knowledge.len(), 1);
        assert_eq!(knowledge[0].0, "fact");
        assert!(context.is_some());
        assert_eq!(domain, Some("database".to_string()));
    }
}
