use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

/// Hash the tool arguments for lookup. Exact match only in v0.2.
/// v0.3: normalize shell commands before hashing.
fn hash_arguments(arguments: &serde_json::Value) -> String {
    let normalized = serde_json::to_string(arguments).unwrap_or_default();
    let hash = Sha256::digest(normalized.as_bytes());
    format!("{:x}", hash)
}

pub struct PreviousResult {
    pub result: String,
    pub success: bool,
    pub created_at: String,
}

/// Look up a recent result for the same tool call (within 24 hours).
pub fn lookup_recent(
    conn: &Connection,
    agent_id: &str,
    tool_name: &str,
    arguments: &serde_json::Value,
) -> anyhow::Result<Option<PreviousResult>> {
    let hash = hash_arguments(arguments);
    let result = conn.query_row(
        "SELECT result, success, created_at FROM tool_results
         WHERE agent_id = ?1 AND tool_name = ?2 AND arguments_hash = ?3
           AND created_at > datetime('now', '-24 hours')
         ORDER BY created_at DESC LIMIT 1",
        params![agent_id, tool_name, hash],
        |row| {
            Ok(PreviousResult {
                result: row.get(0)?,
                success: row.get::<_, i64>(1)? == 1,
                created_at: row.get(2)?,
            })
        },
    );
    match result {
        Ok(r) => Ok(Some(r)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Store a tool result for future lookup.
pub fn store_result(
    conn: &Connection,
    agent_id: &str,
    tool_name: &str,
    arguments: &serde_json::Value,
    result: &str,
    success: bool,
) -> anyhow::Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let hash = hash_arguments(arguments);
    let args_str = serde_json::to_string(arguments).unwrap_or_default();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO tool_results (id, agent_id, tool_name, arguments_hash, arguments, result, success, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, agent_id, tool_name, hash, args_str, result, success as i64, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_store_and_lookup() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let args = serde_json::json!({"command": "ls -la"});
        store_result(&conn, &agent.id, "shell", &args, "file1\nfile2", true).expect("store");
        let prev = lookup_recent(&conn, &agent.id, "shell", &args).expect("lookup");
        assert!(prev.is_some());
        assert_eq!(prev.unwrap().result, "file1\nfile2");
    }

    #[test]
    fn test_lookup_no_match() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let args = serde_json::json!({"command": "ls -la"});
        let prev = lookup_recent(&conn, &agent.id, "shell", &args).expect("lookup");
        assert!(prev.is_none());
    }

    #[test]
    fn test_failed_result_stored() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let args = serde_json::json!({"command": "curl bad-url"});
        store_result(&conn, &agent.id, "shell", &args, "Connection refused", false).expect("store");
        let prev = lookup_recent(&conn, &agent.id, "shell", &args).expect("lookup");
        assert!(prev.is_some());
        let p = prev.unwrap();
        assert!(!p.success);
        assert!(p.result.contains("Connection refused"));
    }

    #[test]
    fn test_different_args_no_match() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        store_result(&conn, &agent.id, "shell", &serde_json::json!({"command": "ls"}), "ok", true).expect("store");
        let prev = lookup_recent(&conn, &agent.id, "shell", &serde_json::json!({"command": "pwd"})).expect("lookup");
        assert!(prev.is_none());
    }
}
