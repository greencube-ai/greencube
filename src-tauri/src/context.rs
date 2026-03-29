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

/// Append to agent's working context (for reflection auto-updates).
pub fn append_context(conn: &Connection, agent_id: &str, text: &str) -> anyhow::Result<()> {
    let current = get_context(conn, agent_id)?;
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
        set_context(&conn, &agent.id, "Line 1").expect("set");
        append_context(&conn, &agent.id, "Line 2").expect("append");
        assert_eq!(get_context(&conn, &agent.id).expect("get"), "Line 1\nLine 2");
    }
}
