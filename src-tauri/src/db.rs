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
    if version < 3 {
        migrate_v2_to_v3(conn)?;
    }
    if version < 4 {
        migrate_v3_to_v4(conn)?;
    }
    if version < 5 {
        migrate_v4_to_v5(conn)?;
    }
    if version < 6 {
        migrate_v5_to_v6(conn)?;
    }
    if version < 7 {
        migrate_v6_to_v7(conn)?;
    }
    if version < 8 {
        migrate_v7_to_v8(conn)?;
    }
    if version < 9 {
        migrate_v8_to_v9(conn)?;
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

/// v2 → v3: Idle thoughts, goals, notifications, metrics, capabilities
fn migrate_v2_to_v3(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        r#"
        -- Agent idle thoughts
        CREATE TABLE IF NOT EXISTS idle_thoughts (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            content TEXT NOT NULL,
            thought_type TEXT NOT NULL DEFAULT 'insight',
            created_at TEXT NOT NULL,
            acted_on INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_idle_thoughts_agent ON idle_thoughts(agent_id, created_at DESC);

        -- Self-directed goals
        CREATE TABLE IF NOT EXISTS agent_goals (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            content TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            progress_notes TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_goals_agent ON agent_goals(agent_id, status);

        -- Agent-initiated notifications
        CREATE TABLE IF NOT EXISTS notifications (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            content TEXT NOT NULL,
            notification_type TEXT NOT NULL DEFAULT 'insight',
            read INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            source TEXT,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_notifications_unread ON notifications(read, created_at DESC);

        -- Growth metrics (daily snapshots)
        CREATE TABLE IF NOT EXISTS agent_metrics (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            date TEXT NOT NULL,
            total_tasks INTEGER NOT NULL,
            successful_tasks INTEGER NOT NULL,
            knowledge_count INTEGER NOT NULL DEFAULT 0,
            total_spend_cents INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE,
            UNIQUE(agent_id, date)
        );

        -- Capability-based discovery
        CREATE TABLE IF NOT EXISTS agent_capabilities (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            capability TEXT NOT NULL,
            confidence REAL NOT NULL DEFAULT 0.5,
            source TEXT NOT NULL DEFAULT 'auto',
            created_at TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_capabilities_search ON agent_capabilities(capability);
    "#,
    )?;

    set_version(conn, 3)?;
    tracing::info!("Database migrated to v3");
    Ok(())
}

/// v3 → v4: Task queue, agent messages, time features
fn migrate_v3_to_v4(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        r#"
        -- Task queue for auto-resume and commitments
        CREATE TABLE IF NOT EXISTS task_queue (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            messages_json TEXT NOT NULL,
            provider_id TEXT,
            resume_at TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'queued',
            retry_count INTEGER NOT NULL DEFAULT 0,
            error_info TEXT,
            source TEXT NOT NULL DEFAULT 'rate_limit',
            prompt TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_task_queue_status ON task_queue(status, resume_at);

        -- Agent-to-agent messages
        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            from_agent_id TEXT NOT NULL,
            to_agent_id TEXT NOT NULL,
            content TEXT NOT NULL,
            message_type TEXT NOT NULL DEFAULT 'request',
            response_content TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (from_agent_id) REFERENCES agents(id) ON DELETE CASCADE,
            FOREIGN KEY (to_agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_messages_agents ON messages(from_agent_id, to_agent_id, created_at DESC);
    "#,
    )?;
    set_version(conn, 4)?;
    tracing::info!("Database migrated to v4");
    Ok(())
}

/// v4 → v5: Journal, competence map, feedback, projects, patterns
fn migrate_v4_to_v5(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        r#"
        -- Daily journal (narrative synthesis)
        CREATE TABLE IF NOT EXISTS journal_entries (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            date TEXT NOT NULL,
            content TEXT NOT NULL,
            task_count INTEGER NOT NULL DEFAULT 0,
            highlights TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE,
            UNIQUE(agent_id, date)
        );

        -- Competence map (domain-specific confidence)
        CREATE TABLE IF NOT EXISTS competence_map (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            domain TEXT NOT NULL,
            confidence REAL NOT NULL DEFAULT 0.5,
            task_count INTEGER NOT NULL DEFAULT 0,
            success_count INTEGER NOT NULL DEFAULT 0,
            last_assessed TEXT NOT NULL,
            trend TEXT NOT NULL DEFAULT 'stable',
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE,
            UNIQUE(agent_id, domain)
        );

        -- Feedback signals (praise/correction)
        CREATE TABLE IF NOT EXISTS feedback_signals (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            signal_type TEXT NOT NULL,
            content TEXT NOT NULL,
            source_task_id TEXT,
            created_at TEXT NOT NULL,
            applied INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_feedback_agent ON feedback_signals(agent_id, created_at DESC);

        -- Project workspaces
        CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            context TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE,
            UNIQUE(agent_id, name)
        );

        -- Task patterns (anticipation)
        CREATE TABLE IF NOT EXISTS task_patterns (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            pattern_description TEXT NOT NULL,
            frequency INTEGER NOT NULL DEFAULT 1,
            last_seen TEXT NOT NULL,
            preparation_note TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
    "#,
    )?;

    // Add project_id to knowledge if not present
    let has_project_id: bool = conn.prepare("SELECT project_id FROM knowledge LIMIT 0").is_ok();
    if !has_project_id {
        conn.execute_batch("ALTER TABLE knowledge ADD COLUMN project_id TEXT REFERENCES projects(id);")?;
    }

    set_version(conn, 5)?;
    tracing::info!("Database migrated to v5");
    Ok(())
}

/// v5 → v6: Response ratings, token usage tracking
fn migrate_v5_to_v6(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS response_ratings (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            task_id TEXT,
            rating INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS token_usage (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            category TEXT NOT NULL,
            tokens INTEGER NOT NULL,
            date TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_token_usage ON token_usage(agent_id, date, category);
    "#)?;
    set_version(conn, 6)?;
    tracing::info!("Database migrated to v6");
    Ok(())
}

/// v6 → v7: Agent self-replication (spawn_specialist)
fn migrate_v6_to_v7(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS agent_lineage (
            id TEXT PRIMARY KEY,
            parent_id TEXT,
            child_id TEXT NOT NULL,
            domain TEXT NOT NULL,
            knowledge_transferred INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            FOREIGN KEY (parent_id) REFERENCES agents(id) ON DELETE SET NULL,
            FOREIGN KEY (child_id) REFERENCES agents(id) ON DELETE CASCADE
        );
    "#)?;

    // Add domain column to knowledge for accurate transfer
    let has_domain: bool = conn.prepare("SELECT domain FROM knowledge LIMIT 0").is_ok();
    if !has_domain {
        conn.execute_batch("ALTER TABLE knowledge ADD COLUMN domain TEXT;")?;
    }

    set_version(conn, 7)?;
    tracing::info!("Database migrated to v7");
    Ok(())
}

/// v7 → v8: Add all tools to existing agents
fn migrate_v7_to_v8(conn: &Connection) -> anyhow::Result<()> {
    use rusqlite::params;

    let all_tools = serde_json::json!([
        "shell", "read_file", "write_file", "http_get",
        "update_context", "set_reminder", "send_message", "spawn_specialist", "fork_agent"
    ]).to_string();

    // Get all agents and update their tools_allowed to include all tools
    let mut stmt = conn.prepare("SELECT id, tools_allowed FROM agents")?;
    let agents: Vec<(String, String)> = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?.collect::<Result<Vec<_>, _>>()?;

    for (id, current_tools_json) in &agents {
        let current: Vec<String> = serde_json::from_str(current_tools_json).unwrap_or_default();
        let all: Vec<String> = serde_json::from_str(&all_tools).unwrap_or_default();

        // Merge: keep existing + add missing
        let mut merged = current.clone();
        for tool in &all {
            if !merged.contains(tool) {
                merged.push(tool.clone());
            }
        }

        let merged_json = serde_json::to_string(&merged).unwrap_or_default();
        conn.execute(
            "UPDATE agents SET tools_allowed = ?1 WHERE id = ?2",
            params![merged_json, id],
        )?;
    }

    set_version(conn, 8)?;
    tracing::info!("Database migrated to v8: all agents now have all tools");
    Ok(())
}

/// v8 → v9: Add valence to knowledge entries + task_patterns table
fn migrate_v8_to_v9(conn: &Connection) -> anyhow::Result<()> {
    // Add valence column to knowledge
    let has_valence: bool = conn.prepare("SELECT valence FROM knowledge LIMIT 0").is_ok();
    if !has_valence {
        conn.execute_batch("ALTER TABLE knowledge ADD COLUMN valence INTEGER DEFAULT 0;")?;
    }

    // Task timing patterns table — drop old v4 schema and recreate with correct columns
    conn.execute_batch(r#"
        DROP TABLE IF EXISTS task_patterns;
        CREATE TABLE task_patterns (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            domain TEXT NOT NULL,
            day_of_week INTEGER NOT NULL,
            hour INTEGER NOT NULL,
            frequency INTEGER NOT NULL DEFAULT 1,
            last_seen TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_task_patterns_agent ON task_patterns(agent_id);
    "#)?;

    // Agent forks table
    crate::fork::create_forks_table(conn)?;

    // Curiosities table
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS curiosities (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            topic TEXT NOT NULL,
            source_task_id TEXT,
            priority INTEGER NOT NULL DEFAULT 1,
            explored INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_curiosities_agent ON curiosities(agent_id, explored, priority DESC);
    "#)?;

    // Drives table
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS drives (
            agent_id TEXT NOT NULL,
            drive_name TEXT NOT NULL,
            energy REAL NOT NULL DEFAULT 0.0,
            threshold REAL NOT NULL DEFAULT 1.0,
            last_discharged_at TEXT NOT NULL,
            PRIMARY KEY (agent_id, drive_name),
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
    "#)?;

    // Context clusters table
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS context_clusters (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            cluster_name TEXT NOT NULL,
            keywords TEXT NOT NULL,
            task_count INTEGER NOT NULL DEFAULT 1,
            last_seen_at TEXT NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
    "#)?;

    // Relationships table
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS relationships (
            agent_id TEXT NOT NULL,
            user_identifier TEXT NOT NULL,
            interactions INTEGER NOT NULL DEFAULT 0,
            positive_signals INTEGER NOT NULL DEFAULT 0,
            negative_signals INTEGER NOT NULL DEFAULT 0,
            notes TEXT NOT NULL DEFAULT '',
            last_interaction_at TEXT NOT NULL,
            PRIMARY KEY (agent_id, user_identifier),
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
    "#)?;

    set_version(conn, 9)?;
    tracing::info!("Database migrated to v9: the cat release");
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
        // v3 tables
        assert!(tables.contains(&"idle_thoughts".to_string()));
        assert!(tables.contains(&"agent_goals".to_string()));
        assert!(tables.contains(&"notifications".to_string()));
        assert!(tables.contains(&"agent_metrics".to_string()));
        assert!(tables.contains(&"agent_capabilities".to_string()));
        // v4 tables
        assert!(tables.contains(&"task_queue".to_string()));
        assert!(tables.contains(&"messages".to_string()));
        // v5 tables
        assert!(tables.contains(&"journal_entries".to_string()));
        assert!(tables.contains(&"competence_map".to_string()));
        assert!(tables.contains(&"feedback_signals".to_string()));
        assert!(tables.contains(&"projects".to_string()));
        assert!(tables.contains(&"task_patterns".to_string()));
        // v6 tables
        assert!(tables.contains(&"response_ratings".to_string()));
        assert!(tables.contains(&"token_usage".to_string()));
        // v7 tables
        assert!(tables.contains(&"agent_lineage".to_string()));
    }

    #[test]
    fn test_schema_version_is_8() {
        let conn = init_memory_database().expect("init");
        let version = get_version(&conn).expect("version");
        assert_eq!(version, 9);
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
        // Running again should be a no-op (version is already 3)
        run_migrations(&conn).expect("second should not fail");
        assert_eq!(get_version(&conn).expect("v"), 9);
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
