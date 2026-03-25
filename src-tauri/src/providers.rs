use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub api_base_url: String,
    pub api_key: String,
    pub default_model: String,
    pub provider_type: String,
    pub created_at: String,
}

pub fn create_provider(
    conn: &Connection,
    name: &str,
    api_base_url: &str,
    api_key: &str,
    default_model: &str,
    provider_type: &str,
) -> anyhow::Result<Provider> {
    if name.trim().is_empty() {
        anyhow::bail!("provider name is required");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO providers (id, name, api_base_url, api_key, default_model, provider_type, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, name, api_base_url, api_key, default_model, provider_type, now],
    )?;
    Ok(Provider { id, name: name.into(), api_base_url: api_base_url.into(), api_key: api_key.into(), default_model: default_model.into(), provider_type: provider_type.into(), created_at: now })
}

pub fn list_providers(conn: &Connection) -> anyhow::Result<Vec<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, api_base_url, api_key, default_model, provider_type, created_at FROM providers ORDER BY created_at"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Provider {
            id: row.get(0)?, name: row.get(1)?, api_base_url: row.get(2)?,
            api_key: row.get(3)?, default_model: row.get(4)?, provider_type: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_provider(conn: &Connection, id: &str) -> anyhow::Result<Option<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, api_base_url, api_key, default_model, provider_type, created_at FROM providers WHERE id = ?1"
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(Provider {
            id: row.get(0)?, name: row.get(1)?, api_base_url: row.get(2)?,
            api_key: row.get(3)?, default_model: row.get(4)?, provider_type: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().next())
}

pub fn get_default_provider(conn: &Connection) -> anyhow::Result<Option<Provider>> {
    // Returns the first provider (oldest created) as the default
    let mut stmt = conn.prepare(
        "SELECT id, name, api_base_url, api_key, default_model, provider_type, created_at FROM providers ORDER BY created_at ASC LIMIT 1"
    )?;
    let mut rows = stmt.query_map([], |row| {
        Ok(Provider {
            id: row.get(0)?, name: row.get(1)?, api_base_url: row.get(2)?,
            api_key: row.get(3)?, default_model: row.get(4)?, provider_type: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().next())
}

/// Get the provider for an agent. Falls back to default if agent has no provider_id.
pub fn get_provider_for_agent(conn: &Connection, agent: &crate::identity::Agent) -> anyhow::Result<Provider> {
    if let Some(ref pid) = agent.provider_id {
        if let Some(p) = get_provider(conn, pid)? {
            return Ok(p);
        }
    }
    // Fallback to default provider
    get_default_provider(conn)?
        .ok_or_else(|| anyhow::anyhow!("No providers configured. Add a provider in Settings."))
}

pub fn update_provider(
    conn: &Connection,
    id: &str,
    name: &str,
    api_base_url: &str,
    api_key: &str,
    default_model: &str,
    provider_type: &str,
) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE providers SET name=?1, api_base_url=?2, api_key=?3, default_model=?4, provider_type=?5 WHERE id=?6",
        params![name, api_base_url, api_key, default_model, provider_type, id],
    )?;
    Ok(())
}

pub fn delete_provider(conn: &Connection, id: &str) -> anyhow::Result<()> {
    // Set agents using this provider to NULL (they'll fall back to default)
    conn.execute("UPDATE agents SET provider_id = NULL WHERE provider_id = ?1", params![id])?;
    conn.execute("DELETE FROM providers WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;

    #[test]
    fn test_create_provider() {
        let conn = init_memory_database().expect("init");
        let p = create_provider(&conn, "OpenAI", "https://api.openai.com/v1", "sk-test", "gpt-4o", "openai").expect("create");
        assert_eq!(p.name, "OpenAI");
        assert_eq!(p.provider_type, "openai");
    }

    #[test]
    fn test_list_providers() {
        let conn = init_memory_database().expect("init");
        create_provider(&conn, "P1", "http://a", "", "m1", "openai").expect("c1");
        create_provider(&conn, "P2", "http://b", "", "m2", "ollama").expect("c2");
        let providers = list_providers(&conn).expect("list");
        assert_eq!(providers.len(), 2);
    }

    #[test]
    fn test_get_default_provider() {
        let conn = init_memory_database().expect("init");
        create_provider(&conn, "First", "http://first", "k1", "m1", "openai").expect("c1");
        create_provider(&conn, "Second", "http://second", "k2", "m2", "ollama").expect("c2");
        let default = get_default_provider(&conn).expect("get").expect("exists");
        assert_eq!(default.name, "First"); // Oldest is default
    }

    #[test]
    fn test_update_provider() {
        let conn = init_memory_database().expect("init");
        let p = create_provider(&conn, "Old", "http://old", "", "m", "openai").expect("create");
        update_provider(&conn, &p.id, "New", "http://new", "key", "gpt-5", "openai").expect("update");
        let updated = get_provider(&conn, &p.id).expect("get").expect("exists");
        assert_eq!(updated.name, "New");
        assert_eq!(updated.api_base_url, "http://new");
    }

    #[test]
    fn test_delete_provider() {
        let conn = init_memory_database().expect("init");
        let p = create_provider(&conn, "Del", "http://x", "", "m", "openai").expect("create");
        delete_provider(&conn, &p.id).expect("delete");
        assert!(get_provider(&conn, &p.id).expect("get").is_none());
    }

    #[test]
    fn test_get_provider_for_agent_fallback() {
        let conn = init_memory_database().expect("init");
        create_provider(&conn, "Default", "http://default", "k", "m", "openai").expect("create");
        // Agent with no provider_id falls back to default
        let agent = crate::identity::Agent {
            id: "a1".into(), name: "Bot".into(), created_at: "".into(), updated_at: "".into(),
            status: "idle".into(), system_prompt: "".into(), public_key: vec![], private_key: vec![],
            tools_allowed: vec![], max_spend_cents: 0, total_tasks: 0, successful_tasks: 0,
            total_spend_cents: 0, provider_id: None, dynamic_profile: String::new(),
        };
        let provider = get_provider_for_agent(&conn, &agent).expect("get");
        assert_eq!(provider.name, "Default");
    }
}
