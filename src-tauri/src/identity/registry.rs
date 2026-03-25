use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use rusqlite::{params, Connection};

use crate::identity::Agent;

/// Valid tool names in v0.2
pub const VALID_TOOLS: &[&str] = &[
    "shell", "read_file", "write_file", "http_get",
    "update_context", "set_reminder", "send_message",
    "create_project", "switch_project", "update_project_context",
];

const SELECT_AGENT: &str =
    "SELECT id, name, created_at, updated_at, status, system_prompt, public_key, private_key, tools_allowed, max_spend_cents, total_tasks, successful_tasks, total_spend_cents, provider_id, dynamic_profile";

fn map_agent(row: &rusqlite::Row) -> rusqlite::Result<Agent> {
    let tools_json: String = row.get(8)?;
    Ok(Agent {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
        status: row.get(4)?,
        system_prompt: row.get(5)?,
        public_key: row.get(6)?,
        private_key: row.get(7)?,
        tools_allowed: serde_json::from_str(&tools_json).unwrap_or_default(),
        max_spend_cents: row.get(9)?,
        total_tasks: row.get(10)?,
        successful_tasks: row.get(11)?,
        total_spend_cents: row.get(12)?,
        provider_id: row.get(13)?,
        dynamic_profile: row.get::<_, Option<String>>(14)?.unwrap_or_default(),
    })
}

pub fn create_agent(
    conn: &Connection,
    name: &str,
    system_prompt: &str,
    tools_allowed: &[String],
) -> anyhow::Result<Agent> {
    create_agent_with_provider(conn, name, system_prompt, tools_allowed, None)
}

pub fn create_agent_with_provider(
    conn: &Connection,
    name: &str,
    system_prompt: &str,
    tools_allowed: &[String],
    provider_id: Option<&str>,
) -> anyhow::Result<Agent> {
    if name.trim().is_empty() {
        anyhow::bail!("name is required");
    }

    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM agents WHERE name = ?1",
        params![name],
        |row| row.get(0),
    )?;
    if exists {
        anyhow::bail!("agent with name '{}' already exists", name);
    }

    for tool in tools_allowed {
        if !VALID_TOOLS.contains(&tool.as_str()) {
            anyhow::bail!(
                "invalid tool: {}. Valid tools: {}",
                tool,
                VALID_TOOLS.join(", ")
            );
        }
    }

    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let tools_json = serde_json::to_string(tools_allowed)?;

    conn.execute(
        "INSERT INTO agents (id, name, created_at, updated_at, status, system_prompt, public_key, private_key, tools_allowed, max_spend_cents, provider_id)
         VALUES (?1, ?2, ?3, ?4, 'idle', ?5, ?6, ?7, ?8, 0, ?9)",
        params![
            id, name, now, now, system_prompt,
            verifying_key.as_bytes().to_vec(),
            signing_key.to_bytes().to_vec(),
            tools_json,
            provider_id,
        ],
    )?;

    Ok(Agent {
        id,
        name: name.to_string(),
        created_at: now.clone(),
        updated_at: now,
        status: "idle".to_string(),
        system_prompt: system_prompt.to_string(),
        public_key: verifying_key.as_bytes().to_vec(),
        private_key: signing_key.to_bytes().to_vec(),
        tools_allowed: tools_allowed.to_vec(),
        max_spend_cents: 0,
        total_tasks: 0,
        successful_tasks: 0,
        total_spend_cents: 0,
        provider_id: provider_id.map(|s| s.to_string()),
        dynamic_profile: String::new(),
    })
}

pub fn list_agents(conn: &Connection) -> anyhow::Result<Vec<Agent>> {
    let sql = format!("{} FROM agents ORDER BY created_at DESC", SELECT_AGENT);
    let mut stmt = conn.prepare(&sql)?;
    let agents = stmt.query_map([], map_agent)?.collect::<Result<Vec<_>, _>>()?;
    Ok(agents)
}

pub fn get_agent(conn: &Connection, id: &str) -> anyhow::Result<Option<Agent>> {
    let sql = format!("{} FROM agents WHERE id = ?1", SELECT_AGENT);
    let mut stmt = conn.prepare(&sql)?;
    let agents = stmt.query_map(params![id], map_agent)?.collect::<Result<Vec<_>, _>>()?;
    Ok(agents.into_iter().next())
}

pub fn get_agent_by_name(conn: &Connection, name: &str) -> anyhow::Result<Option<Agent>> {
    let sql = format!("{} FROM agents WHERE name = ?1", SELECT_AGENT);
    let mut stmt = conn.prepare(&sql)?;
    let agents = stmt.query_map(params![name], map_agent)?.collect::<Result<Vec<_>, _>>()?;
    Ok(agents.into_iter().next())
}

pub fn update_agent_status(conn: &Connection, id: &str, status: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE agents SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status, now, id],
    )?;
    Ok(())
}

pub fn update_agent_dynamic_profile(conn: &Connection, id: &str, profile: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE agents SET dynamic_profile = ?1, updated_at = ?2 WHERE id = ?3",
        params![profile, now, id],
    )?;
    Ok(())
}

pub fn increment_task_counts(
    conn: &Connection,
    id: &str,
    success: bool,
    cost_cents: i64,
) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    if success {
        conn.execute(
            "UPDATE agents SET total_tasks = total_tasks + 1, successful_tasks = successful_tasks + 1, total_spend_cents = total_spend_cents + ?1, updated_at = ?2 WHERE id = ?3",
            params![cost_cents, now, id],
        )?;
    } else {
        conn.execute(
            "UPDATE agents SET total_tasks = total_tasks + 1, total_spend_cents = total_spend_cents + ?1, updated_at = ?2 WHERE id = ?3",
            params![cost_cents, now, id],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;

    #[test]
    fn test_create_agent() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "TestBot", "You are helpful.", &["shell".into()]).expect("create");
        assert_eq!(agent.name, "TestBot");
        assert_eq!(agent.status, "idle");
        assert_eq!(agent.provider_id, None);
        assert_eq!(agent.dynamic_profile, "");
    }

    #[test]
    fn test_create_agent_with_provider() {
        let conn = init_memory_database().expect("init");
        let p = crate::providers::create_provider(&conn, "TestProv", "http://x", "k", "m", "openai").expect("provider");
        let agent = create_agent_with_provider(&conn, "ProvBot", "", &["shell".into()], Some(&p.id)).expect("create");
        assert_eq!(agent.provider_id, Some(p.id.clone()));
        let fetched = get_agent(&conn, &agent.id).expect("get").expect("found");
        assert_eq!(fetched.provider_id, Some(p.id));
    }

    #[test]
    fn test_create_agent_generates_keys() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "KeyBot", "", &["shell".into()]).expect("create");
        assert_eq!(agent.public_key.len(), 32);
        assert_eq!(agent.private_key.len(), 32);
    }

    #[test]
    fn test_create_agent_duplicate_name() {
        let conn = init_memory_database().expect("init");
        create_agent(&conn, "DupeBot", "", &["shell".into()]).expect("first");
        let result = create_agent(&conn, "DupeBot", "", &["shell".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_agent_invalid_tool() {
        let conn = init_memory_database().expect("init");
        let result = create_agent(&conn, "BadBot", "", &["nonexistent_tool".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_agent_empty_name() {
        let conn = init_memory_database().expect("init");
        let result = create_agent(&conn, "", "", &["shell".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_agents() {
        let conn = init_memory_database().expect("init");
        create_agent(&conn, "Bot1", "", &["shell".into()]).expect("c1");
        create_agent(&conn, "Bot2", "", &["shell".into()]).expect("c2");
        create_agent(&conn, "Bot3", "", &["shell".into()]).expect("c3");
        assert_eq!(list_agents(&conn).expect("list").len(), 3);
    }

    #[test]
    fn test_get_agent_found() {
        let conn = init_memory_database().expect("init");
        let created = create_agent(&conn, "FindMe", "", &["shell".into()]).expect("create");
        let found = get_agent(&conn, &created.id).expect("get").expect("found");
        assert_eq!(found.name, "FindMe");
    }

    #[test]
    fn test_get_agent_not_found() {
        let conn = init_memory_database().expect("init");
        assert!(get_agent(&conn, "nonexistent").expect("get").is_none());
    }

    #[test]
    fn test_get_agent_by_name() {
        let conn = init_memory_database().expect("init");
        create_agent(&conn, "NamedBot", "", &["shell".into()]).expect("create");
        assert!(get_agent_by_name(&conn, "NamedBot").expect("get").is_some());
        assert!(get_agent_by_name(&conn, "NoSuchBot").expect("get").is_none());
    }

    #[test]
    fn test_reputation_new_agent() {
        let agent = Agent {
            id: "t".into(), name: "t".into(), created_at: "".into(), updated_at: "".into(),
            status: "idle".into(), system_prompt: "".into(), public_key: vec![], private_key: vec![],
            tools_allowed: vec![], max_spend_cents: 0, total_tasks: 0, successful_tasks: 0,
            total_spend_cents: 0, provider_id: None, dynamic_profile: String::new(),
        };
        assert!((agent.reputation() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_reputation_all_success() {
        let agent = Agent {
            id: "t".into(), name: "t".into(), created_at: "".into(), updated_at: "".into(),
            status: "idle".into(), system_prompt: "".into(), public_key: vec![], private_key: vec![],
            tools_allowed: vec![], max_spend_cents: 0, total_tasks: 10, successful_tasks: 10,
            total_spend_cents: 0, provider_id: None, dynamic_profile: String::new(),
        };
        assert!((agent.reputation() - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_reputation_half_success() {
        let agent = Agent {
            id: "t".into(), name: "t".into(), created_at: "".into(), updated_at: "".into(),
            status: "idle".into(), system_prompt: "".into(), public_key: vec![], private_key: vec![],
            tools_allowed: vec![], max_spend_cents: 0, total_tasks: 10, successful_tasks: 5,
            total_spend_cents: 0, provider_id: None, dynamic_profile: String::new(),
        };
        assert!((agent.reputation() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_update_status() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "StatusBot", "", &["shell".into()]).expect("create");
        update_agent_status(&conn, &agent.id, "active").expect("update");
        assert_eq!(get_agent(&conn, &agent.id).expect("get").expect("f").status, "active");
    }

    #[test]
    fn test_increment_task_counts() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "CountBot", "", &["shell".into()]).expect("create");
        increment_task_counts(&conn, &agent.id, true, 50).expect("s");
        increment_task_counts(&conn, &agent.id, false, 30).expect("f");
        let updated = get_agent(&conn, &agent.id).expect("get").expect("f");
        assert_eq!(updated.total_tasks, 2);
        assert_eq!(updated.successful_tasks, 1);
        assert_eq!(updated.total_spend_cents, 80);
    }

    #[test]
    fn test_dynamic_profile_update() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "ProfBot", "", &["shell".into()]).expect("create");
        update_agent_dynamic_profile(&conn, &agent.id, "Expert coder").expect("update");
        let updated = get_agent(&conn, &agent.id).expect("get").expect("f");
        assert_eq!(updated.dynamic_profile, "Expert coder");
    }
}
