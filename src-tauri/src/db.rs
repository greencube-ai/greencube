use rusqlite::Connection;
use std::path::Path;

pub fn init_database(db_path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(db_path)?;

    // Enable WAL mode for better concurrent read performance
    // Using execute_batch as fallback-safe approach across rusqlite versions
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

    conn.execute_batch(SCHEMA_SQL)?;

    Ok(conn)
}

/// Initialize an in-memory database (for tests)
#[cfg(test)]
pub fn init_memory_database() -> anyhow::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}

const SCHEMA_SQL: &str = r#"
    CREATE TABLE IF NOT EXISTS schema_version (
        version INTEGER NOT NULL DEFAULT 1
    );
    INSERT OR IGNORE INTO schema_version (version) VALUES (1);

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
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_creates_all_tables() {
        let conn = init_memory_database().expect("init");
        // Verify all tables exist by querying sqlite_master
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .expect("prepare")
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect");

        assert!(tables.contains(&"agents".to_string()));
        assert!(tables.contains(&"episodes".to_string()));
        assert!(tables.contains(&"audit_log".to_string()));
        assert!(tables.contains(&"config_store".to_string()));
        assert!(tables.contains(&"schema_version".to_string()));
    }

    #[test]
    fn test_init_is_idempotent() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch("PRAGMA foreign_keys=ON;").expect("pragma");
        conn.execute_batch(SCHEMA_SQL).expect("first init");
        conn.execute_batch(SCHEMA_SQL).expect("second init should not fail");
    }

    #[test]
    fn test_foreign_keys_enforced() {
        let conn = init_memory_database().expect("init");
        let result = conn.execute(
            "INSERT INTO episodes (id, agent_id, created_at, event_type, summary)
             VALUES ('ep1', 'nonexistent-agent', '2026-01-01T00:00:00Z', 'test', 'test')",
            [],
        );
        assert!(result.is_err(), "FK constraint should prevent insert with bad agent_id");
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
        assert!(result.is_err(), "UNIQUE constraint should prevent duplicate name");
    }
}
