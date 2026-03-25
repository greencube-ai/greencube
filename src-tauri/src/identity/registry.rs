use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use rusqlite::{params, Connection};

use crate::identity::Agent;

/// Valid tool names in v0.1
pub const VALID_TOOLS: &[&str] = &["shell", "read_file", "write_file", "http_get"];

pub fn create_agent(
    conn: &Connection,
    name: &str,
    system_prompt: &str,
    tools_allowed: &[String],
) -> anyhow::Result<Agent> {
    // Validate name
    if name.trim().is_empty() {
        anyhow::bail!("name is required");
    }

    // Check for duplicate name
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM agents WHERE name = ?1",
        params![name],
        |row| row.get(0),
    )?;
    if exists {
        anyhow::bail!("agent with name '{}' already exists", name);
    }

    // Validate tools
    for tool in tools_allowed {
        if !VALID_TOOLS.contains(&tool.as_str()) {
            anyhow::bail!(
                "invalid tool: {}. Valid tools: {}",
                tool,
                VALID_TOOLS.join(", ")
            );
        }
    }

    // Generate Ed25519 key pair
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let tools_json = serde_json::to_string(tools_allowed)?;

    conn.execute(
        "INSERT INTO agents (id, name, created_at, updated_at, status, system_prompt, public_key, private_key, tools_allowed, max_spend_cents)
         VALUES (?1, ?2, ?3, ?4, 'idle', ?5, ?6, ?7, ?8, 0)",
        params![
            id,
            name,
            now,
            now,
            system_prompt,
            verifying_key.as_bytes().to_vec(),
            signing_key.to_bytes().to_vec(),
            tools_json,
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
    })
}

pub fn list_agents(conn: &Connection) -> anyhow::Result<Vec<Agent>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at, updated_at, status, system_prompt, public_key, private_key, tools_allowed, max_spend_cents, total_tasks, successful_tasks, total_spend_cents
         FROM agents ORDER BY created_at DESC",
    )?;

    let agents = stmt
        .query_map([], |row| {
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
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(agents)
}

pub fn get_agent(conn: &Connection, id: &str) -> anyhow::Result<Option<Agent>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at, updated_at, status, system_prompt, public_key, private_key, tools_allowed, max_spend_cents, total_tasks, successful_tasks, total_spend_cents
         FROM agents WHERE id = ?1",
    )?;

    let mut agents = stmt
        .query_map(params![id], |row| {
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
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(agents.into_iter().next())
}

pub fn get_agent_by_name(conn: &Connection, name: &str) -> anyhow::Result<Option<Agent>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at, updated_at, status, system_prompt, public_key, private_key, tools_allowed, max_spend_cents, total_tasks, successful_tasks, total_spend_cents
         FROM agents WHERE name = ?1",
    )?;

    let mut agents = stmt
        .query_map(params![name], |row| {
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
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

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
        assert_eq!(agent.tools_allowed, vec!["shell"]);
        assert_eq!(agent.total_tasks, 0);
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
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn test_create_agent_invalid_tool() {
        let conn = init_memory_database().expect("init");
        let result = create_agent(&conn, "BadBot", "", &["nonexistent_tool".into()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid tool"));
    }

    #[test]
    fn test_create_agent_empty_name() {
        let conn = init_memory_database().expect("init");
        let result = create_agent(&conn, "", "", &["shell".into()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("name is required"));
    }

    #[test]
    fn test_list_agents() {
        let conn = init_memory_database().expect("init");
        create_agent(&conn, "Bot1", "", &["shell".into()]).expect("create1");
        create_agent(&conn, "Bot2", "", &["shell".into()]).expect("create2");
        create_agent(&conn, "Bot3", "", &["shell".into()]).expect("create3");
        let agents = list_agents(&conn).expect("list");
        assert_eq!(agents.len(), 3);
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
        let found = get_agent(&conn, "nonexistent-id").expect("get");
        assert!(found.is_none());
    }

    #[test]
    fn test_get_agent_by_name() {
        let conn = init_memory_database().expect("init");
        create_agent(&conn, "NamedBot", "", &["shell".into()]).expect("create");
        let found = get_agent_by_name(&conn, "NamedBot").expect("get").expect("found");
        assert_eq!(found.name, "NamedBot");
        let not_found = get_agent_by_name(&conn, "NoSuchBot").expect("get");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_reputation_new_agent() {
        let agent = Agent {
            id: "test".into(), name: "test".into(), created_at: "".into(), updated_at: "".into(),
            status: "idle".into(), system_prompt: "".into(), public_key: vec![], private_key: vec![],
            tools_allowed: vec![], max_spend_cents: 0, total_tasks: 0, successful_tasks: 0, total_spend_cents: 0,
        };
        assert!((agent.reputation() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_reputation_all_success() {
        let agent = Agent {
            id: "test".into(), name: "test".into(), created_at: "".into(), updated_at: "".into(),
            status: "idle".into(), system_prompt: "".into(), public_key: vec![], private_key: vec![],
            tools_allowed: vec![], max_spend_cents: 0, total_tasks: 10, successful_tasks: 10, total_spend_cents: 0,
        };
        assert!((agent.reputation() - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_reputation_half_success() {
        let agent = Agent {
            id: "test".into(), name: "test".into(), created_at: "".into(), updated_at: "".into(),
            status: "idle".into(), system_prompt: "".into(), public_key: vec![], private_key: vec![],
            tools_allowed: vec![], max_spend_cents: 0, total_tasks: 10, successful_tasks: 5, total_spend_cents: 0,
        };
        assert!((agent.reputation() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_update_status() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "StatusBot", "", &["shell".into()]).expect("create");
        update_agent_status(&conn, &agent.id, "active").expect("update");
        let updated = get_agent(&conn, &agent.id).expect("get").expect("found");
        assert_eq!(updated.status, "active");
    }

    #[test]
    fn test_increment_task_counts() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "CountBot", "", &["shell".into()]).expect("create");
        increment_task_counts(&conn, &agent.id, true, 50).expect("inc success");
        increment_task_counts(&conn, &agent.id, false, 30).expect("inc failure");
        let updated = get_agent(&conn, &agent.id).expect("get").expect("found");
        assert_eq!(updated.total_tasks, 2);
        assert_eq!(updated.successful_tasks, 1);
        assert_eq!(updated.total_spend_cents, 80);
    }
}
