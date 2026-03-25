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

/// Set (replace) agent's working context. Truncates to 1000 chars.
pub fn set_context(conn: &Connection, agent_id: &str, content: &str) -> anyhow::Result<()> {
    let truncated: String = content.chars().take(1000).collect();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO agent_context (agent_id, content, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(agent_id) DO UPDATE SET content = ?2, updated_at = ?3",
        params![agent_id, truncated, now],
    )?;
    Ok(())
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
        assert_eq!(get_context(&conn, &agent.id).expect("get").len(), 1000);
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
