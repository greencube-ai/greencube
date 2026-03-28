use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub agent_id: String,
    pub name: String,
    pub description: String,
    pub context: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

pub fn create_project(conn: &Connection, agent_id: &str, name: &str, description: &str) -> anyhow::Result<Project> {
    if name.trim().is_empty() { anyhow::bail!("project name is required"); }
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO projects (id, agent_id, name, description, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6)",
        params![id, agent_id, name, description, now, now],
    )?;
    Ok(Project { id, agent_id: agent_id.into(), name: name.into(), description: description.into(), context: String::new(), status: "active".into(), created_at: now.clone(), updated_at: now })
}

pub fn list_projects(conn: &Connection, agent_id: &str) -> anyhow::Result<Vec<Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, name, description, context, status, created_at, updated_at FROM projects WHERE agent_id = ?1 ORDER BY updated_at DESC"
    )?;
    let projects = stmt.query_map(params![agent_id], |row| {
        Ok(Project {
            id: row.get(0)?, agent_id: row.get(1)?, name: row.get(2)?,
            description: row.get(3)?, context: row.get(4)?, status: row.get(5)?,
            created_at: row.get(6)?, updated_at: row.get(7)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(projects)
}

pub fn get_active_project(conn: &Connection, agent_id: &str) -> anyhow::Result<Option<Project>> {
    let key = format!("active_project_{}", agent_id);
    let project_id: String = match conn.query_row(
        "SELECT value FROM config_store WHERE key = ?1", params![key], |row| row.get(0),
    ) {
        Ok(id) => id,
        Err(_) => return Ok(None),
    };
    if project_id.is_empty() { return Ok(None); }

    let result = conn.query_row(
        "SELECT id, agent_id, name, description, context, status, created_at, updated_at FROM projects WHERE id = ?1",
        params![project_id],
        |row| Ok(Project {
            id: row.get(0)?, agent_id: row.get(1)?, name: row.get(2)?,
            description: row.get(3)?, context: row.get(4)?, status: row.get(5)?,
            created_at: row.get(6)?, updated_at: row.get(7)?,
        }),
    );
    match result {
        Ok(p) => Ok(Some(p)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn set_active_project(conn: &Connection, agent_id: &str, project_name: &str) -> anyhow::Result<String> {
    // Find project by name
    let project_id: String = conn.query_row(
        "SELECT id FROM projects WHERE agent_id = ?1 AND name = ?2",
        params![agent_id, project_name],
        |row| row.get(0),
    ).map_err(|_| anyhow::anyhow!("Project '{}' not found", project_name))?;

    let key = format!("active_project_{}", agent_id);
    conn.execute(
        "INSERT INTO config_store (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![key, project_id],
    )?;
    Ok(project_id)
}

pub fn update_project_context(conn: &Connection, agent_id: &str, project_name: &str, context: &str) -> anyhow::Result<()> {
    let truncated: String = context.chars().take(2000).collect();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE projects SET context = ?1, updated_at = ?2 WHERE agent_id = ?3 AND name = ?4",
        params![truncated, now, agent_id, project_name],
    )?;
    Ok(())
}

/// Get knowledge entries tagged with a specific project.
pub fn get_project_knowledge(conn: &Connection, project_id: &str) -> anyhow::Result<Vec<crate::knowledge::KnowledgeEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, content, source_task_id, category, confidence, created_at, last_used_at, use_count, COALESCE(valence, 0)
         FROM knowledge WHERE project_id = ?1 ORDER BY created_at DESC LIMIT 10"
    )?;
    let entries = stmt.query_map(params![project_id], |row| {
        Ok(crate::knowledge::KnowledgeEntry {
            id: row.get(0)?, agent_id: row.get(1)?, content: row.get(2)?,
            source_task_id: row.get(3)?, category: row.get(4)?, confidence: row.get(5)?,
            created_at: row.get(6)?, last_used_at: row.get(7)?, use_count: row.get(8)?,
            valence: row.get(9)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;
    use crate::identity::registry::create_agent;

    #[test]
    fn test_create_project() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        let project = create_project(&conn, &agent.id, "Payment API", "Stripe integration").expect("create");
        assert_eq!(project.name, "Payment API");
        assert_eq!(project.status, "active");
    }

    #[test]
    fn test_set_and_get_active_project() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        create_project(&conn, &agent.id, "MyProject", "desc").expect("create");
        set_active_project(&conn, &agent.id, "MyProject").expect("set");
        let active = get_active_project(&conn, &agent.id).expect("get").expect("exists");
        assert_eq!(active.name, "MyProject");
    }

    #[test]
    fn test_update_project_context() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        create_project(&conn, &agent.id, "P1", "").expect("create");
        update_project_context(&conn, &agent.id, "P1", "API auth done. Webhooks pending.").expect("update");
        set_active_project(&conn, &agent.id, "P1").expect("set");
        let active = get_active_project(&conn, &agent.id).expect("get").expect("exists");
        assert_eq!(active.context, "API auth done. Webhooks pending.");
    }

    #[test]
    fn test_no_active_project() {
        let conn = init_memory_database().expect("init");
        let agent = create_agent(&conn, "Bot", "", &["shell".into()]).expect("create");
        assert!(get_active_project(&conn, &agent.id).expect("get").is_none());
    }
}
