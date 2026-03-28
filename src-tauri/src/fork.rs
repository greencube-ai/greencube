use std::sync::Arc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::identity::registry;
use crate::knowledge;
use crate::providers;
use crate::state::AppState;

const MAX_ACTIVE_FORKS: i64 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkRecord {
    pub id: String,
    pub parent_id: String,
    pub branch_label: String,
    pub branch_description: String,
    pub task_id: String,
    pub response: Option<String>,
    pub selected: bool,
    pub created_at: String,
}

/// Execute a fork: create two temporary branches, run both, pick the winner.
pub async fn execute_fork(
    state: &AppState,
    parent_agent_id: &str,
    reason: &str,
    branch_a_desc: &str,
    branch_b_desc: &str,
    original_messages: &[serde_json::Value],
) -> anyhow::Result<String> {
    // Check fork limits
    {
        let db = state.db.lock().await;
        let active: i64 = db.query_row(
            "SELECT COUNT(*) FROM agent_forks WHERE parent_id = ?1 AND response IS NULL",
            params![parent_agent_id],
            |row| row.get(0),
        ).unwrap_or(0);
        if active >= MAX_ACTIVE_FORKS {
            anyhow::bail!("Maximum {} active forks reached. Wait for current forks to complete.", MAX_ACTIVE_FORKS);
        }
    }

    // Get parent agent and provider
    let (parent, provider) = {
        let db = state.db.lock().await;
        let parent = registry::get_agent(&db, parent_agent_id)?
            .ok_or_else(|| anyhow::anyhow!("Parent agent not found"))?;
        let provider = providers::get_provider_for_agent(&db, &parent)?;
        (parent, provider)
    };

    let task_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // Record both forks
    let fork_a_id = uuid::Uuid::new_v4().to_string();
    let fork_b_id = uuid::Uuid::new_v4().to_string();
    {
        let db = state.db.lock().await;
        db.execute(
            "INSERT INTO agent_forks (id, parent_id, branch_label, branch_description, task_id, selected, created_at)
             VALUES (?1, ?2, 'A', ?3, ?4, 0, ?5)",
            params![fork_a_id, parent_agent_id, branch_a_desc, task_id, now],
        )?;
        db.execute(
            "INSERT INTO agent_forks (id, parent_id, branch_label, branch_description, task_id, selected, created_at)
             VALUES (?1, ?2, 'B', ?3, ?4, 0, ?5)",
            params![fork_b_id, parent_agent_id, branch_b_desc, task_id, now],
        )?;
    }

    if let Some(handle) = &state.app_handle {
        let _ = handle.emit("activity-refresh", ());
    }

    tracing::info!("Fork initiated by {}: A='{}' vs B='{}'", parent.name, branch_a_desc, branch_b_desc);

    // Build messages for each branch
    let mut messages_a = original_messages.to_vec();
    let mut messages_b = original_messages.to_vec();

    // Inject branch-specific system prompt
    let base_system = parent.system_prompt.clone();

    if let Some(sys) = messages_a.iter_mut().find(|m| m["role"] == "system") {
        if let Some(c) = sys["content"].as_str() {
            sys["content"] = serde_json::Value::String(format!("{}\n\nYou are exploring APPROACH A: {}", c, branch_a_desc));
        }
    } else {
        messages_a.insert(0, serde_json::json!({"role": "system", "content": format!("{}\n\nYou are exploring APPROACH A: {}", base_system, branch_a_desc)}));
    }

    if let Some(sys) = messages_b.iter_mut().find(|m| m["role"] == "system") {
        if let Some(c) = sys["content"].as_str() {
            sys["content"] = serde_json::Value::String(format!("{}\n\nYou are exploring APPROACH B: {}", c, branch_b_desc));
        }
    } else {
        messages_b.insert(0, serde_json::json!({"role": "system", "content": format!("{}\n\nYou are exploring APPROACH B: {}", base_system, branch_b_desc)}));
    }

    // Run both branches concurrently
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", provider.api_base_url);

    let (resp_a, resp_b) = tokio::join!(
        call_llm(&client, &url, &provider.api_key, &provider.default_model, &messages_a),
        call_llm(&client, &url, &provider.api_key, &provider.default_model, &messages_b),
    );

    let response_a = resp_a.unwrap_or_else(|e| format!("Branch A failed: {}", e));
    let response_b = resp_b.unwrap_or_else(|e| format!("Branch B failed: {}", e));

    // Store responses
    {
        let db = state.db.lock().await;
        let _ = db.execute("UPDATE agent_forks SET response = ?1 WHERE id = ?2", params![response_a, fork_a_id]);
        let _ = db.execute("UPDATE agent_forks SET response = ?1 WHERE id = ?2", params![response_b, fork_b_id]);
    }

    // Ask parent to pick the winner
    let judge_prompt = format!(
        "You tried two approaches for: {}\n\nApproach A ({}):\n{}\n\nApproach B ({}):\n{}\n\nWhich approach produced a better result? Reply with EXACTLY: WINNER: A or WINNER: B, followed by a one-sentence explanation.",
        reason, branch_a_desc, &response_a[..response_a.len().min(500)], branch_b_desc, &response_b[..response_b.len().min(500)]
    );

    let judge_messages = vec![
        serde_json::json!({"role": "system", "content": "You are an AI assistant."}),
        serde_json::json!({"role": "user", "content": judge_prompt}),
    ];

    let judge_result = call_llm(&client, &url, &provider.api_key, &provider.default_model, &judge_messages).await
        .unwrap_or_else(|_| "WINNER: A".to_string());

    let winner_is_a = !judge_result.contains("WINNER: B");
    let winner_label = if winner_is_a { "A" } else { "B" };
    let winner_response = if winner_is_a { &response_a } else { &response_b };
    let winner_desc = if winner_is_a { branch_a_desc } else { branch_b_desc };

    // Mark winner
    {
        let db = state.db.lock().await;
        let winner_id = if winner_is_a { &fork_a_id } else { &fork_b_id };
        let _ = db.execute("UPDATE agent_forks SET selected = 1 WHERE id = ?1", params![winner_id]);

        // Store knowledge about the fork result
        let _ = knowledge::insert_knowledge(
            &db, parent_agent_id,
            &format!("Forked to try two approaches for '{}'. Approach {} ({}) won. {}", reason, winner_label, winner_desc, judge_result.chars().take(100).collect::<String>()),
            "fact", Some(&task_id),
        );

        // Log episode
        let _ = crate::memory::episodic::insert_episode(&db, &crate::memory::Episode {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: parent_agent_id.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            event_type: "fork".into(),
            summary: format!("Forked: {} vs {} — {} won", branch_a_desc, branch_b_desc, winner_label),
            raw_data: Some(judge_result.clone()),
            task_id: Some(task_id), outcome: Some("success".into()),
            tokens_used: 0, cost_cents: 0,
        });
    }

    if let Some(handle) = &state.app_handle {
        let _ = handle.emit("activity-refresh", ());
    }

    tracing::info!("Fork complete: {} won for '{}'", winner_label, reason);

    Ok(format!("[Forked: tried {} vs {}. {} won.]\n\n{}", branch_a_desc, branch_b_desc, winner_label, winner_response))
}

async fn call_llm(client: &reqwest::Client, url: &str, api_key: &str, model: &str, messages: &[serde_json::Value]) -> anyhow::Result<String> {
    let resp = client.post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&serde_json::json!({
            "model": model,
            "messages": messages,
            "max_tokens": 1000,
        }))
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("LLM call failed: {}", err);
    }

    let body: serde_json::Value = resp.json().await?;
    Ok(body["choices"][0]["message"]["content"].as_str().unwrap_or("No response").to_string())
}

/// Create the agent_forks table if it doesn't exist (called from migration).
pub fn create_forks_table(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS agent_forks (
            id TEXT PRIMARY KEY,
            parent_id TEXT NOT NULL,
            branch_label TEXT NOT NULL,
            branch_description TEXT NOT NULL,
            task_id TEXT NOT NULL,
            response TEXT,
            selected INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            FOREIGN KEY (parent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
    "#)?;
    Ok(())
}
