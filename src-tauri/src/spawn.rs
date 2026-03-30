use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::competence;
use crate::context;
use crate::identity::registry;
use crate::knowledge;
use crate::memory::episodic;
use crate::memory::Episode;
use crate::notifications;
use crate::providers;
use crate::state::AppState;

const MAX_CHILDREN: i64 = 3;
const MAX_TOTAL_AGENTS: i64 = 10; // SECURITY: Global cap on total agents to prevent runaway spawning
const MIN_DOMAIN_TASKS: i64 = 5; // 5 tasks in a domain before spawning is considered
const MIN_COMPETENCE_GAP: f64 = 0.20; // spawn when domain is 20+ percentage points below best domain
const MIN_TOTAL_TASKS: i64 = 10; // 10 total tasks before any spawning

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageInfo {
    pub parent: Option<(String, String, String)>, // (id, name, domain)
    pub children: Vec<(String, String, String, i64)>, // (id, name, domain, knowledge_count)
}

/// Check if an agent can spawn (used by both idle thinker and tool).
/// SECURITY: Checks both per-agent child limit AND global agent cap.
/// Also prevents children from spawning (no recursive spawning in v0.7).
pub fn can_spawn(conn: &Connection, agent_id: &str) -> bool {
    // Prevent children from spawning their own children
    let is_child: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM agent_lineage WHERE child_id = ?1",
        params![agent_id],
        |row| row.get(0),
    ).unwrap_or(false);
    if is_child {
        return false;
    }

    // Per-agent child limit
    if count_children(conn, agent_id) >= MAX_CHILDREN {
        return false;
    }

    // Global agent cap
    let total_agents: i64 = conn.query_row(
        "SELECT COUNT(*) FROM agents",
        [],
        |row| row.get(0),
    ).unwrap_or(0);
    total_agents < MAX_TOTAL_AGENTS
}

pub fn count_children(conn: &Connection, parent_id: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM agent_lineage WHERE parent_id = ?1",
        params![parent_id],
        |row| row.get(0),
    ).unwrap_or(0)
}

pub fn get_children(conn: &Connection, parent_id: &str) -> Vec<(String, String, String, i64)> {
    let mut stmt = conn.prepare(
        "SELECT al.child_id, a.name, al.domain, al.knowledge_transferred
         FROM agent_lineage al JOIN agents a ON a.id = al.child_id
         WHERE al.parent_id = ?1"
    ).unwrap_or_else(|_| panic!("prepare children query"));
    stmt.query_map(params![parent_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?))
    }).unwrap_or_else(|_| panic!("query children")).collect::<Result<Vec<_>, _>>().unwrap_or_default()
}

pub fn get_parent(conn: &Connection, child_id: &str) -> Option<(String, String, String)> {
    conn.query_row(
        "SELECT al.parent_id, a.name, al.domain
         FROM agent_lineage al JOIN agents a ON a.id = al.parent_id
         WHERE al.child_id = ?1",
        params![child_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
    ).ok()
}

pub fn get_lineage(conn: &Connection, agent_id: &str) -> LineageInfo {
    LineageInfo {
        parent: get_parent(conn, agent_id),
        children: get_children(conn, agent_id),
    }
}

/// The main spawn function. Creates a specialist child agent.
pub async fn execute_spawn(
    state: &AppState,
    parent_agent_id: &str,
    domain: &str,
) -> anyhow::Result<String> {
    let domain = domain.to_lowercase().trim().to_string();

    // 1. VALIDATE (brief DB lock)
    let (parent, provider, competence_entry, domain_knowledge, domain_feedback) = {
        let db = state.db.lock().await;

        // Get parent
        let parent = registry::get_agent(&db, parent_agent_id)?
            .ok_or_else(|| anyhow::anyhow!("Parent agent not found"))?;

        // Check total tasks
        if parent.total_tasks < MIN_TOTAL_TASKS {
            anyhow::bail!("Need at least {} completed tasks before spawning (have {})", MIN_TOTAL_TASKS, parent.total_tasks);
        }

        // Check children count
        if !can_spawn(&db, parent_agent_id) {
            anyhow::bail!("Maximum {} specialist agents reached", MAX_CHILDREN);
        }

        // Check competence in domain — spawn only when there's a RELATIVE weakness
        let comp_map = competence::get_competence_map(&db, parent_agent_id)?;
        let entry = comp_map.iter().find(|c| c.domain == domain)
            .ok_or_else(|| anyhow::anyhow!("No competence data for domain '{}'", domain))?;

        if entry.task_count < MIN_DOMAIN_TASKS {
            anyhow::bail!("Need at least {} tasks in '{}' before spawning (have {})", MIN_DOMAIN_TASKS, domain, entry.task_count);
        }

        // Find the best domain (with enough tasks) to compare against
        let best_confidence = comp_map.iter()
            .filter(|c| c.task_count >= MIN_DOMAIN_TASKS)
            .map(|c| c.confidence)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(entry.confidence);

        let gap = best_confidence - entry.confidence;
        if gap < MIN_COMPETENCE_GAP {
            anyhow::bail!(
                "Competence in '{}' is {}% — only {}% below best domain ({}%). Need {}%+ gap to spawn specialist.",
                domain, (entry.confidence * 100.0) as i64,
                (gap * 100.0) as i64, (best_confidence * 100.0) as i64,
                (MIN_COMPETENCE_GAP * 100.0) as i64
            );
        }

        let entry_clone = entry.clone();

        // Get provider
        let provider = providers::get_provider_for_agent(&db, &parent)?;

        // Gather domain knowledge (by domain tag, not keyword)
        let all_knowledge = knowledge::list_knowledge(&db, parent_agent_id, 100)?;
        let domain_k: Vec<_> = all_knowledge.into_iter()
            .filter(|k| {
                // Match by domain tag if set, else fallback to keyword match
                k.content.to_lowercase().contains(&domain)
                    || k.category == "warning"
            })
            .take(20)
            .collect();

        // Gather domain feedback
        let all_feedback = crate::feedback::get_recent_feedback(&db, parent_agent_id, 50)?;
        let domain_f: Vec<_> = all_feedback.into_iter()
            .filter(|f| f.content.to_lowercase().contains(&domain))
            .take(10)
            .collect();

        (parent, provider, entry_clone, domain_k, domain_f)
    };
    // DB lock released

    // 2. GENERATE SYSTEM PROMPT (LLM call, no lock)
    let knowledge_text = domain_knowledge.iter()
        .map(|k| format!("- [{}] {}", k.category, k.content))
        .collect::<Vec<_>>().join("\n");
    let warnings = domain_knowledge.iter()
        .filter(|k| k.category == "warning")
        .map(|k| k.content.clone())
        .take(3)
        .collect::<Vec<_>>();
    let feedback_text = domain_feedback.iter()
        .map(|f| format!("- [{}] {}", f.signal_type, f.content))
        .collect::<Vec<_>>().join("\n");

    let gen_prompt = format!(
        r#"Write a system prompt for a specialist agent in {domain}. Max 3 sentences.
Knowledge from parent agent:
{knowledge}
{feedback_section}
The specialist should focus on accuracy in {domain}, be aware of past failures, and flag uncertainty.
Write ONLY the system prompt, nothing else."#,
        domain = domain,
        knowledge = if knowledge_text.is_empty() { "No specific knowledge yet." } else { &knowledge_text },
        feedback_section = if feedback_text.is_empty() { String::new() } else { format!("Feedback:\n{}", feedback_text) },
    );

    let generated_prompt = match reqwest::Client::new()
        .post(format!("{}/chat/completions", provider.api_base_url))
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .json(&serde_json::json!({
            "model": provider.default_model,
            "messages": [
                {"role": "system", "content": "You are an AI assistant."},
                {"role": "user", "content": gen_prompt}
            ],
            "max_tokens": 200,
            "temperature": 0.3,
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send().await
    {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let text = body["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string();
            if text.is_empty() {
                format!("You are a specialist in {}. Focus on accuracy and flag uncertainty.", domain)
            } else {
                text.chars().take(500).collect() // Cap at 500 tokens worth
            }
        }
        _ => format!("You are a specialist in {}. Focus on accuracy and flag uncertainty.", domain),
    };

    // 3. CREATE CHILD AGENT (brief DB lock)
    let child_name = {
        let db = state.db.lock().await;

        // Determine unique name
        let base_name = format!("{}-specialist", domain);
        let mut name = base_name.clone();
        let mut suffix = 1;
        while registry::get_agent_by_name(&db, &name)?.is_some() {
            suffix += 1;
            name = format!("{}-{}", base_name, suffix);
        }

        // Remove spawn_specialist from child's tools (no recursive spawning)
        let child_tools: Vec<String> = parent.tools_allowed.iter()
            .filter(|t| t.as_str() != "spawn_specialist")
            .cloned()
            .collect();

        // Create the child
        let child = registry::create_agent_with_provider(
            &db, &name, &generated_prompt, &child_tools, parent.provider_id.as_deref(),
        )?;

        // Transfer knowledge (new entries with child's agent_id)
        let mut transferred = 0i64;
        for k in &domain_knowledge {
            let _ = knowledge::insert_knowledge(&db, &child.id, &k.content, &k.category, k.source_task_id.as_deref());
            transferred += 1;
        }

        // Seed child's scratchpad
        let warnings_text = if warnings.is_empty() {
            "No specific warnings.".to_string()
        } else {
            warnings.iter().map(|w| format!("- {}", w)).collect::<Vec<_>>().join("\n")
        };
        context::set_context(&db, &child.id, &format!(
            "You were created by {} to specialize in {}.\nTransferred: {} knowledge entries.\nKey warnings:\n{}",
            parent.name, domain, transferred, warnings_text
        ))?;

        // Record lineage
        let lineage_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(
            "INSERT INTO agent_lineage (id, parent_id, child_id, domain, knowledge_transferred, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![lineage_id, parent_agent_id, child.id, domain, transferred, now],
        )?;

        // Add delegation note to parent's knowledge
        let _ = knowledge::insert_knowledge(
            &db, parent_agent_id,
            &format!("[delegation] {} tasks delegated to {}. Use send_message to delegate.", domain, name),
            "fact", None,
        );

        // Notification
        let notify_msg = format!(
            "{} created a specialist: {} ({} knowledge entries transferred). {} uses the same provider as {}. You can change this in agent settings.",
            parent.name, name, transferred, name, parent.name,
        );
        let _ = notifications::create_notification(&db, parent_agent_id, &notify_msg, "achievement", "spawn");

        // Log episode
        let _ = episodic::insert_episode(&db, &Episode {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: parent_agent_id.into(),
            created_at: now,
            event_type: "spawn".into(),
            summary: format!("Spawned {} for {} ({} knowledge entries transferred)", name, domain, transferred),
            raw_data: None,
            task_id: None,
            outcome: Some("success".into()),
            tokens_used: 0,
            cost_cents: 0,
        });

        name
    };

    // 4. EMIT EVENTS
    if let Some(handle) = &state.app_handle {
        let _ = handle.emit("notification-new", serde_json::json!({"agent_id": parent_agent_id}));
        let _ = handle.emit("activity-refresh", ());
    }

    tracing::info!("Agent {} spawned specialist {} for domain {}", parent.name, child_name, domain);
    Ok(child_name)
}

/// Debug-only: force spawn a specialist without competence validation.
/// Used by the test button in the UI. Remove before real launch.
pub async fn debug_force_spawn(
    state: &AppState,
    parent_agent_id: &str,
    domain: &str,
) -> anyhow::Result<String> {
    let domain = domain.to_lowercase().trim().to_string();

    let (parent, provider) = {
        let db = state.db.lock().await;
        let parent = registry::get_agent(&db, parent_agent_id)?
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;
        let provider = providers::get_provider_for_agent(&db, &parent)?;
        (parent, provider)
    };

    // Generate a simple system prompt
    let prompt = format!("You are a specialist in {}. Focus on accuracy and flag uncertainty.", domain);

    // Create child agent
    let child_name = {
        let db = state.db.lock().await;
        let name = format!("{}-specialist", domain);
        let tools: Vec<String> = parent.tools_allowed.iter()
            .filter(|t| *t != "spawn_specialist")
            .cloned().collect();
        let child = registry::create_agent_with_provider(&db, &name, &prompt, &tools, parent.provider_id.as_deref())?;

        // Create lineage record
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(
            "INSERT INTO agent_lineage (id, parent_id, child_id, domain, knowledge_transferred, created_at) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            params![uuid::Uuid::new_v4().to_string(), parent_agent_id, child.id, domain, now],
        )?;

        // Log episode
        let _ = episodic::insert_episode(&db, &Episode {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: parent_agent_id.into(),
            created_at: now,
            event_type: "spawn".into(),
            summary: format!("Debug spawned {} for {}", name, domain),
            raw_data: None, task_id: None,
            outcome: Some("success".into()),
            tokens_used: 0, cost_cents: 0,
        });

        name
    };

    if let Some(handle) = &state.app_handle {
        let _ = handle.emit("activity-refresh", ());
    }

    tracing::info!("Debug spawn: {} created {} for {}", parent.name, child_name, domain);
    Ok(child_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_database;

    #[test]
    fn test_can_spawn_initially() {
        let conn = init_memory_database().expect("init");
        let agent = registry::create_agent(&conn, "Dev", "", &["shell".into()]).expect("create");
        assert!(can_spawn(&conn, &agent.id));
    }

    #[test]
    fn test_count_children_zero() {
        let conn = init_memory_database().expect("init");
        let agent = registry::create_agent(&conn, "Dev", "", &["shell".into()]).expect("create");
        assert_eq!(count_children(&conn, &agent.id), 0);
    }

    #[test]
    fn test_lineage_record() {
        let conn = init_memory_database().expect("init");
        let parent = registry::create_agent(&conn, "Dev", "", &["shell".into()]).expect("parent");
        let child = registry::create_agent(&conn, "css-specialist", "", &["shell".into()]).expect("child");
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO agent_lineage (id, parent_id, child_id, domain, knowledge_transferred, created_at) VALUES ('l1', ?1, ?2, 'css', 5, ?3)",
            params![parent.id, child.id, now],
        ).expect("insert");

        let children = get_children(&conn, &parent.id);
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].1, "css-specialist");
        assert_eq!(children[0].2, "css");
        assert_eq!(children[0].3, 5);

        let parent_info = get_parent(&conn, &child.id);
        assert!(parent_info.is_some());
        assert_eq!(parent_info.unwrap().1, "Dev");
    }

    #[test]
    fn test_max_children_enforced() {
        let conn = init_memory_database().expect("init");
        let parent = registry::create_agent(&conn, "Dev", "", &["shell".into()]).expect("parent");
        let now = chrono::Utc::now().to_rfc3339();
        for i in 0..3 {
            let child = registry::create_agent(&conn, &format!("child-{}", i), "", &["shell".into()]).expect("child");
            conn.execute(
                "INSERT INTO agent_lineage (id, parent_id, child_id, domain, knowledge_transferred, created_at) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
                params![format!("l{}", i), parent.id, child.id, format!("domain-{}", i), now],
            ).expect("insert");
        }
        assert!(!can_spawn(&conn, &parent.id)); // Max 3 reached
    }

    #[test]
    fn test_lineage_info() {
        let conn = init_memory_database().expect("init");
        let parent = registry::create_agent(&conn, "Dev", "", &["shell".into()]).expect("parent");
        let child = registry::create_agent(&conn, "css-spec", "", &["shell".into()]).expect("child");
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO agent_lineage (id, parent_id, child_id, domain, knowledge_transferred, created_at) VALUES ('l1', ?1, ?2, 'css', 3, ?3)",
            params![parent.id, child.id, now],
        ).expect("insert");

        let parent_lineage = get_lineage(&conn, &parent.id);
        assert!(parent_lineage.parent.is_none());
        assert_eq!(parent_lineage.children.len(), 1);

        let child_lineage = get_lineage(&conn, &child.id);
        assert!(child_lineage.parent.is_some());
        assert_eq!(child_lineage.children.len(), 0);
    }

    #[test]
    fn test_child_cannot_spawn() {
        // SECURITY: Children must not be able to spawn their own children
        let conn = init_memory_database().expect("init");
        let parent = registry::create_agent(&conn, "Dev", "", &["shell".into()]).expect("parent");
        let child = registry::create_agent(&conn, "css-spec", "", &["shell".into()]).expect("child");
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO agent_lineage (id, parent_id, child_id, domain, knowledge_transferred, created_at) VALUES ('l1', ?1, ?2, 'css', 3, ?3)",
            params![parent.id, child.id, now],
        ).expect("insert");

        // Parent can spawn (has fewer than MAX_CHILDREN)
        assert!(can_spawn(&conn, &parent.id));
        // Child CANNOT spawn (is itself a child)
        assert!(!can_spawn(&conn, &child.id));
    }

    #[test]
    fn test_global_agent_cap() {
        // SECURITY: Total agents across the system must not exceed MAX_TOTAL_AGENTS
        let conn = init_memory_database().expect("init");
        // Create 10 agents (the global cap)
        for i in 0..10 {
            registry::create_agent(&conn, &format!("agent-{}", i), "", &["shell".into()]).expect("create");
        }
        // The first agent shouldn't be able to spawn because we're at the global cap
        assert!(!can_spawn(&conn, "agent-0"));
    }
}
