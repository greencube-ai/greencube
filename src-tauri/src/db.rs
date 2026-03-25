use rusqlite::{params, Connection};
use std::path::Path;

pub fn init_database(db_path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    // Ensure schema_version table exists
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL DEFAULT 0);",
    )?;
    // Insert version 0 if table is empty
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))?;
    if count == 0 {
        conn.execute("INSERT INTO schema_version (version) VALUES (0)", [])?;
    }
    run_migrations(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn init_memory_database() -> anyhow::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL DEFAULT 0);",
    )?;
    conn.execute("INSERT INTO schema_version (version) VALUES (0)", [])?;
    run_migrations(&conn)?;
    Ok(conn)
}

fn get_version(conn: &Connection) -> anyhow::Result<i64> {
    let v: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |r| r.get(0),
    )?;
    Ok(v)
}

fn set_version(conn: &Connection, version: i64) -> anyhow::Result<()> {
    conn.execute("UPDATE schema_version SET version = ?1", params![version])?;
    Ok(())
}

fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
    let version = get_version(conn)?;
    if version < 1 {
        migrate_v0_to_v1(conn)?;
    }
    if version < 2 {
        migrate_v1_to_v2(conn)?;
    }
    Ok(())
}

/// v0 → v1: Original schema (agents, episodes, audit_log, config_store)
fn migrate_v0_to_v1(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS agents (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'idle',
            system_prompt TEXT NOT NULL DEFAULT '',
            public_key BLOB NOT NULL,
            private_key BLOB NOT NULL,
            tools_allowed TEXT NOT NULL DEFAULT '[]',
            max_spend_cents INTEGER NOT NULL DEFAULT 0,
            total_tasks INTEGER NOT NULL DEFAULT 0,
            successful_tasks INTEGER NOT NULL DEFAULT 0,
            total_spend_cents INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS episodes (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            event_type TEXT NOT NULL,
            summary TEXT NOT NULL,
            raw_data TEXT,
            task_id TEXT,
            outcome TEXT,
            tokens_used INTEGER DEFAULT 0,
            cost_cents INTEGER DEFAULT 0,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_episodes_agent_time ON episodes(agent_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_episodes_task ON episodes(task_id);

        CREATE TABLE IF NOT EXISTS audit_log (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            action_type TEXT NOT NULL,
            action_detail TEXT NOT NULL,
            permission_result TEXT NOT NULL,
            result TEXT,
            duration_ms INTEGER,
            cost_cents INTEGER DEFAULT 0,
            error TEXT,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_audit_agent_time ON audit_log(agent_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_audit_type ON audit_log(action_type);

        CREATE TABLE IF NOT EXISTS config_store (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
    "#,
    )?;
    set_version(conn, 1)?;
    tracing::info!("Database migrated to v1");
    Ok(())
}

/// v1 → v2: Multi-provider, knowledge, scratchpad, tool memory, dynamic profile
fn migrate_v1_to_v2(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        r#"
        -- Multi-provider support
        CREATE TABLE IF NOT EXISTS providers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            api_base_url TEXT NOT NULL,
            api_key TEXT NOT NULL DEFAULT '',
            default_model TEXT NOT NULL DEFAULT 'gpt-4o',
            provider_type TEXT NOT NULL DEFAULT 'openai',
            created_at TEXT NOT NULL
        );

        -- Knowledge base (structured learning)
        CREATE TABLE IF NOT EXISTS knowledge (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            content TEXT NOT NULL,
            source_task_id TEXT,
            category TEXT NOT NULL DEFAULT 'general',
            confidence REAL NOT NULL DEFAULT 1.0,
            created_at TEXT NOT NULL,
            last_used_at TEXT,
            use_count INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_knowledge_agent ON knowledge(agent_id);

        -- Agent working context (scratchpad)
        CREATE TABLE IF NOT EXISTS agent_context (
            agent_id TEXT PRIMARY KEY,
            content TEXT NOT NULL DEFAULT '',
            updated_at TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );

        -- Tool result memory
        CREATE TABLE IF NOT EXISTS tool_results (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            tool_name TEXT NOT NULL,
            arguments_hash TEXT NOT NULL,
            arguments TEXT NOT NULL,
            result TEXT NOT NULL,
            success INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_tool_results_lookup ON tool_results(agent_id, tool_name, arguments_hash);
    "#,
    )?;

    // Add provider_id and dynamic_profile columns to agents (if not already present)
    // SQLite doesn't have IF NOT EXISTS for ALTER TABLE, so check first
    let has_provider_id: bool = conn
        .prepare("SELECT provider_id FROM agents LIMIT 0")
        .is_ok();
    if !has_provider_id {
        conn.execute_batch("ALTER TABLE agents ADD COLUMN provider_id TEXT REFERENCES providers(id);")?;
    }

    let has_dynamic_profile: bool = conn
        .prepare("SELECT dynamic_profile FROM agents LIMIT 0")
        .is_ok();
    if !has_dynamic_profile {
        conn.execute_batch(
            "ALTER TABLE agents ADD COLUMN dynamic_profile TEXT NOT NULL DEFAULT '';",
        )?;
    }

    // Seed default provider from config.toml if it exists and has an API key
    // This is done by the caller (main.rs) after migration, since we don't have config access here

    set_version(conn, 2)?;
    tracing::info!("Database migrated to v2");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_creates_all_tables() {
        let conn = init_memory_database().expect("init");
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .expect("prepare")
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect");

        // v1 tables
        assert!(tables.contains(&"agents".to_string()));
        assert!(tables.contains(&"episodes".to_string()));
        assert!(tables.contains(&"audit_log".to_string()));
        assert!(tables.contains(&"config_store".to_string()));
        assert!(tables.contains(&"schema_version".to_string()));
        // v2 tables
        assert!(tables.contains(&"providers".to_string()));
        assert!(tables.contains(&"knowledge".to_string()));
        assert!(tables.contains(&"agent_context".to_string()));
        assert!(tables.contains(&"tool_results".to_string()));
    }

    #[test]
    fn test_schema_version_is_2() {
        let conn = init_memory_database().expect("init");
        let version = get_version(&conn).expect("version");
        assert_eq!(version, 2);
    }

    #[test]
    fn test_init_is_idempotent() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch("PRAGMA foreign_keys=ON;").expect("pragma");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL DEFAULT 0);",
        )
        .expect("sv");
        conn.execute("INSERT INTO schema_version (version) VALUES (0)", [])
            .expect("insert");
        run_migrations(&conn).expect("first");
        // Running again should be a no-op (version is already 2)
        run_migrations(&conn).expect("second should not fail");
        assert_eq!(get_version(&conn).expect("v"), 2);
    }

    #[test]
    fn test_foreign_keys_enforced() {
        let conn = init_memory_database().expect("init");
        let result = conn.execute(
            "INSERT INTO episodes (id, agent_id, created_at, event_type, summary)
             VALUES ('ep1', 'nonexistent-agent', '2026-01-01T00:00:00Z', 'test', 'test')",
            [],
        );
        assert!(
            result.is_err(),
            "FK constraint should prevent insert with bad agent_id"
        );
    }

    #[test]
    fn test_unique_agent_name() {
        let conn = init_memory_database().expect("init");
        conn.execute(
            "INSERT INTO agents (id, name, created_at, updated_at, public_key, private_key)
             VALUES ('a1', 'TestBot', '2026-01-01', '2026-01-01', X'00', X'00')",
            [],
        )
        .expect("first insert");
        let result = conn.execute(
            "INSERT INTO agents (id, name, created_at, updated_at, public_key, private_key)
             VALUES ('a2', 'TestBot', '2026-01-01', '2026-01-01', X'00', X'00')",
            [],
        );
        assert!(
            result.is_err(),
            "UNIQUE constraint should prevent duplicate name"
        );
    }

    #[test]
    fn test_agents_have_new_columns() {
        let conn = init_memory_database().expect("init");
        // Insert agent with new columns
        conn.execute(
            "INSERT INTO agents (id, name, created_at, updated_at, public_key, private_key, provider_id, dynamic_profile)
             VALUES ('a1', 'Bot', '2026-01-01', '2026-01-01', X'00', X'00', NULL, 'A helpful bot')",
            [],
        )
        .expect("insert with new columns");
        let profile: String = conn
            .query_row("SELECT dynamic_profile FROM agents WHERE id='a1'", [], |r| {
                r.get(0)
            })
            .expect("query");
        assert_eq!(profile, "A helpful bot");
    }
}
