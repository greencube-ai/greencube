use rusqlite::{params, Connection};

/// Get agent's working context (scratchpad). Returns empty string if none set.
pub fn get_context(conn: &Connection, agent_id: &str) -> anyhow::Result<String> {
    let result = conn.query_row(
        "SELECT content FROM agent_context WHERE agent_id = ?1",
        params![agent_id],
        |row| row.get::<_, String>(0),
    );
    match result {
        Ok(content) => Ok(content),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

/// Set (replace) agent's working context. Smart compaction at 1000 chars:
/// keeps first 200 chars (foundational context) + last 800 chars (most recent).
pub fn set_context(conn: &Connection, agent_id: &str, content: &str) -> anyhow::Result<()> {
    let char_count = content.chars().count();
    let truncated = if char_count > 1000 {
        let first: String = content.chars().take(200).collect();
        let last: String = content.chars().skip(char_count - 800).collect();
        format!("{}\n...\n{}", first, last)
    } else {
        content.to_string()
    };
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO agent_context (agent_id, content, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(agent_id) DO UPDATE SET content = ?2, updated_at = ?3",
        params![agent_id, truncated, now],
    )?;
    Ok(())
}

/// Deduplicate scratchpad lines, keeping most recent occurrence of each.
pub fn compact_context(conn: &Connection, agent_id: &str) -> anyhow::Result<()> {
    let current = get_context(conn, agent_id)?;
    if current.is_empty() { return Ok(()); }
    let lines: Vec<&str> = current.lines().collect();
    let mut seen = std::collections::HashSet::new();
    // Walk from end to start, keeping first (most recent) occurrence
    let deduped: Vec<&str> = lines.iter().rev()
        .filter(|line| {
            let key = line.trim().to_lowercase();
            if key.len() < 10 { return true; } // keep short lines
            seen.insert(key)
        })
        .copied()
        .collect::<Vec<_>>().into_iter().rev().collect();
    set_context(conn, agent_id, &deduped.join("\n"))
}

/// Generic reflection summary patterns that add no value to the scratchpad.
const JUNK_PATTERNS: &[&str] = &[
    "last reflection:",
    "entries extracted",
    "domain: general",
    "context updated",
    "context unchanged",
];

/// Append to agent's working context, but only if the content is genuinely new.
/// Skips generic reflection summaries and lines that are 80%+ similar to existing lines.
pub fn append_context(conn: &Connection, agent_id: &str, text: &str) -> anyhow::Result<()> {
    let lower = text.to_lowercase();

    // Skip generic/junk lines
    if JUNK_PATTERNS.iter().any(|p| lower.contains(p)) {
        return Ok(());
    }

    let current = get_context(conn, agent_id)?;

    // Check if a similar line already exists (80%+ word overlap = skip)
    if !current.is_empty() {
        let new_words: std::collections::HashSet<&str> = text.split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|w| w.len() > 2)
            .collect();

        if !new_words.is_empty() {
            for existing_line in current.lines() {
                let existing_words: std::collections::HashSet<&str> = existing_line.split_whitespace()
                    .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
                    .filter(|w| w.len() > 2)
                    .collect();
                if existing_words.is_empty() { continue; }

                let overlap = new_words.intersection(&existing_words).count();
                let max_len = new_words.len().max(existing_words.len());
                if max_len > 0 && (overlap as f64 / max_len as f64) >= 0.8 {
                    return Ok(()); // too similar, skip
                }
            }
        }
    }

    let new_content = if current.is_empty() {
        text.to_string()
    } else {
        format!("{}\n{}", current, text)
    };
    set_context(conn, agent_id, &new_content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_get_empty_context() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let ctx = get_context(&conn, &agent.id).expect("get");
        assert_eq!(ctx, "");
    }

    #[test]
    fn test_set_and_get_context() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        set_context(&conn, &agent.id, "Working on payment API").expect("set");
        let ctx = get_context(&conn, &agent.id).expect("get");
        assert_eq!(ctx, "Working on payment API");
    }

    #[test]
    fn test_context_replace() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        set_context(&conn, &agent.id, "first").expect("set1");
        set_context(&conn, &agent.id, "second").expect("set2");
        assert_eq!(get_context(&conn, &agent.id).expect("get"), "second");
    }

    #[test]
    fn test_context_truncation() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let long = "x".repeat(2000);
        set_context(&conn, &agent.id, &long).expect("set");
        let result = get_context(&conn, &agent.id).expect("get");
        // Smart compaction: first 200 + "\n...\n" + last 800 = 1005
        assert!(result.len() <= 1010);
        assert!(result.contains("..."));
    }

    #[test]
    fn test_append_context() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        set_context(&conn, &agent.id, "Working on payment integration").expect("set");
        append_context(&conn, &agent.id, "Self-verify: BAD in css — incomplete auth handling").expect("append");
        let ctx = get_context(&conn, &agent.id).expect("get");
        assert!(ctx.contains("payment integration"));
        assert!(ctx.contains("Self-verify: BAD"));
    }

    #[test]
    fn test_append_skips_similar() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        set_context(&conn, &agent.id, "Self-verify: BAD in css — incomplete auth handling").expect("set");
        // Same content, should be skipped
        append_context(&conn, &agent.id, "Self-verify: BAD in css — incomplete auth handling").expect("append");
        let ctx = get_context(&conn, &agent.id).expect("get");
        assert_eq!(ctx.matches("Self-verify").count(), 1); // only one occurrence
    }

    #[test]
    fn test_append_skips_junk() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        set_context(&conn, &agent.id, "Working on auth").expect("set");
        append_context(&conn, &agent.id, "Last reflection: 3 entries extracted. Domain: python.").expect("append");
        let ctx = get_context(&conn, &agent.id).expect("get");
        assert!(!ctx.contains("entries extracted")); // junk was filtered
    }
}
