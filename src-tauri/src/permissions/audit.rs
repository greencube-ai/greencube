use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub agent_id: String,
    pub created_at: String,
    pub action_type: String,
    pub action_detail: String,
    pub permission_result: String,
    pub result: Option<String>,
    pub duration_ms: Option<i64>,
    pub cost_cents: i64,
    pub error: Option<String>,
}

pub fn log_action(conn: &Connection, entry: &AuditEntry) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO audit_log (id, agent_id, created_at, action_type, action_detail, permission_result, result, duration_ms, cost_cents, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            entry.id,
            entry.agent_id,
            entry.created_at,
            entry.action_type,
            entry.action_detail,
            entry.permission_result,
            entry.result,
            entry.duration_ms,
            entry.cost_cents,
            entry.error,
        ],
    )?;
    Ok(())
}

pub fn get_audit_log(
    conn: &Connection,
    agent_id: &str,
    limit: i64,
) -> anyhow::Result<Vec<AuditEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, created_at, action_type, action_detail, permission_result, result, duration_ms, cost_cents, error
         FROM audit_log WHERE agent_id = ?1 ORDER BY created_at DESC LIMIT ?2",
    )?;

    let entries = stmt
        .query_map(params![agent_id, limit], map_audit_entry)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(entries)
}

/// Get recent activity across ALL agents (for dashboard feed)
pub fn get_recent_activity(conn: &Connection, limit: i64) -> anyhow::Result<Vec<AuditEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, created_at, action_type, action_detail, permission_result, result, duration_ms, cost_cents, error
         FROM audit_log ORDER BY created_at DESC LIMIT ?1",
    )?;

    let entries = stmt
        .query_map(params![limit], map_audit_entry)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(entries)
}

fn map_audit_entry(row: &rusqlite::Row) -> rusqlite::Result<AuditEntry> {
    Ok(AuditEntry {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        created_at: row.get(2)?,
        action_type: row.get(3)?,
        action_detail: row.get(4)?,
        permission_result: row.get(5)?,
        result: row.get(6)?,
        duration_ms: row.get(7)?,
        cost_cents: row.get(8)?,
        error: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    fn make_entry(agent_id: &str, id: &str, action_type: &str) -> AuditEntry {
        AuditEntry {
            id: id.into(),
            agent_id: agent_id.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            action_type: action_type.into(),
            action_detail: r#"{"tool":"shell","command":"ls"}"#.into(),
            permission_result: "allowed".into(),
            result: Some("ok".into()),
            duration_ms: Some(100),
            cost_cents: 5,
            error: None,
        }
    }

    #[test]
    fn test_log_and_retrieve_audit() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "AuditBot", "", &["shell".into()]).expect("create");
        let entry = make_entry(&agent.id, "audit1", "tool_call");
        log_action(&conn, &entry).expect("log");

        let entries = get_audit_log(&conn, &agent.id, 50).expect("get");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].action_type, "tool_call");
    }

    #[test]
    fn test_audit_ordering() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "OrderBot", "", &["shell".into()]).expect("create");

        let mut e1 = make_entry(&agent.id, "a1", "tool_call");
        e1.created_at = "2026-01-01T00:00:00Z".into();
        log_action(&conn, &e1).expect("log");

        let mut e2 = make_entry(&agent.id, "a2", "llm_request");
        e2.created_at = "2026-01-02T00:00:00Z".into();
        log_action(&conn, &e2).expect("log");

        let entries = get_audit_log(&conn, &agent.id, 50).expect("get");
        assert_eq!(entries.len(), 2);
        // Newest first
        assert_eq!(entries[0].action_type, "llm_request");
        assert_eq!(entries[1].action_type, "tool_call");
    }

    #[test]
    fn test_recent_activity() {
        let conn = init_memory_database().expect("init");
        let a1 = create_agent(&conn, "Bot1", "", &["shell".into()]).expect("create");
        let a2 = create_agent(&conn, "Bot2", "", &["shell".into()]).expect("create");

        log_action(&conn, &make_entry(&a1.id, "e1", "tool_call")).expect("log");
        log_action(&conn, &make_entry(&a2.id, "e2", "llm_request")).expect("log");

        let entries = get_recent_activity(&conn, 50).expect("get");
        assert_eq!(entries.len(), 2);
    }
}
