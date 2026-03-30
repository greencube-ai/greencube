use std::sync::Arc;
use tauri::Emitter;
use crate::knowledge;
use crate::memory::episodic;
use crate::memory::Episode;
use crate::providers::Provider;
use crate::state::AppState;

const REFLECTION_PROMPT: &str = r#"Review the conversation above. Extract what you learned.

You MUST format each learning as EXACTLY one of these tags on its own line:
[fact valence=N] statement here
[warning valence=N] statement here
[preference] statement here
[curious] a question or topic you want to explore later (something interesting from this task)
[domain] one-word category of this task (e.g., python, css, database, api, devops)
[context] brief note for your scratchpad

Valence is your emotional memory: -2=very frustrating, -1=difficult, 0=neutral, +1=went well, +2=excellent.
Do NOT number the items. Just the tags, one per line.
Always include exactly one [domain] tag.
If something in this conversation made you curious, include a [curious] tag.
If nothing was learned, write only: NONE

Example output:
[fact valence=1] The user's API uses Bearer token authentication
[warning valence=-2] The /v2 endpoint returns 404 and was very frustrating to debug
[curious] what happens when JWT tokens expire mid-request?
[domain] api
[context] Working on payment integration, auth is done"#;

/// Run self-reflection after a task completes. Spawns as a background task.
pub fn spawn_reflection(
    state: Arc<AppState>,
    agent_id: String,
    provider: Provider,
    messages: Vec<serde_json::Value>,
    task_id: String,
) {
    tokio::spawn(async move {
        if let Err(e) = run_reflection(&state, &agent_id, &provider, &messages, &task_id).await {
            tracing::warn!("Reflection failed for agent {}: {}", agent_id, e);
        }
    });
}

async fn run_reflection(
    state: &AppState,
    agent_id: &str,
    provider: &Provider,
    messages: &[serde_json::Value],
    task_id: &str,
) -> anyhow::Result<()> {
    // Budget check: skip if daily background token budget exceeded
    {
        let db = state.db.lock().await;
        let budget = state.config.read().await.cost.daily_background_token_budget;
        if !crate::token_usage::has_budget_remaining(&db, agent_id, 500, budget)? {
            tracing::info!("Budget exceeded, skipping reflection for agent {}", agent_id);
            let _ = crate::permissions::audit::log_action(&db, &crate::permissions::audit::AuditEntry {
                id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id.into(),
                created_at: chrono::Utc::now().to_rfc3339(), action_type: "budget_skip".into(),
                action_detail: "Skipped reflection: daily background token budget exceeded".into(),
                permission_result: "denied".into(), result: None, duration_ms: None, cost_cents: 0, error: None,
            });
            return Ok(());
        }
    }

    // Build a condensed summary of the conversation (limit to avoid huge prompts)
    let mut summary_messages: Vec<serde_json::Value> = messages.iter()
        .filter(|m| {
            let role = m["role"].as_str().unwrap_or("");
            role == "user" || role == "assistant"
        })
        .take(10) // Max 10 messages to keep reflection prompt reasonable
        .cloned()
        .collect();


    // Add the reflection prompt as the final user message
    summary_messages.push(serde_json::json!({
        "role": "user",
        "content": REFLECTION_PROMPT
    }));

    // Call the agent's provider (non-streaming, low cost)
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", provider.api_base_url);

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": provider.default_model,
            "messages": summary_messages,
            "max_tokens": 500,
            "temperature": 0.3,
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        // Log to audit trail so the user can see it
        let db = state.db.lock().await;
        let _ = crate::permissions::audit::log_action(&db, &crate::permissions::audit::AuditEntry {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            action_type: "background_error".into(),
            action_detail: format!("Reflection failed: HTTP {} — {}", status, &error_text[..error_text.len().min(200)]),
            permission_result: "allowed".into(),
            result: None, duration_ms: None, cost_cents: 0,
            error: Some(format!("HTTP {}", status)),
        });
        anyhow::bail!("Reflection LLM call failed: HTTP {} — {}", status, error_text);
    }

    let body: serde_json::Value = response.json().await?;

    // Record token usage for budget tracking
    let tokens_used = body["usage"]["total_tokens"].as_i64().unwrap_or(500);
    {
        let db = state.db.lock().await;
        let _ = crate::token_usage::record_usage(&db, agent_id, "reflection", tokens_used);
    }

    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("NONE");

    if content.trim() == "NONE" {
        return Ok(());
    }

    // Parse the reflection response
    let (knowledge_entries, context_update, domain) = knowledge::parse_reflection_response(content);

    let db = state.db.lock().await;

    // Store knowledge entries with valence, route curiosities to curiosity queue
    for (category, entry_content, valence) in &knowledge_entries {
        if category == "curious" {
            let _ = crate::curiosity::add_curiosity(&db, agent_id, entry_content, Some(task_id));
        } else {
            let _ = knowledge::insert_knowledge_with_valence(&db, agent_id, entry_content, category, Some(task_id), *valence);
        }
    }

    // Update working context if provided
    if let Some(ctx) = &context_update {
        let _ = crate::context::append_context(&db, agent_id, ctx);
    }

    // Only write actionable content to scratchpad (not generic summaries)
    // The [context] tag from reflection already writes genuinely useful notes above

    // Compact scratchpad if it's getting long — dedup and smart truncation
    let ctx_len = crate::context::get_context(&db, agent_id).map(|c| c.len()).unwrap_or(0);
    if ctx_len > 800 {
        let _ = crate::context::compact_context(&db, agent_id);
    }

    // Memory decay: mark low-relevance knowledge as stale
    let _ = knowledge::mark_stale_entries(&db, agent_id);

    // Update context cluster for this domain
    if let Some(ref d) = domain {
        let _ = crate::context_clusters::update_cluster(&db, agent_id, d);
    }

    // Charge curiosity drive if [curious] entries were extracted
    let curious_count = knowledge_entries.iter().filter(|(c, _, _)| c == "curious").count();
    if curious_count > 0 {
        let _ = crate::drives::charge_drive(&db, agent_id, "curiosity", 0.3 * curious_count as f64);
    }

    // Update competence + task patterns for this domain
    if let Some(ref d) = domain {
        let _ = crate::competence::update_competence(&db, agent_id, d, true, None);
        let _ = crate::task_patterns::record_task_timing(&db, agent_id, d);
        tracing::info!("Competence updated: agent {} domain '{}' (success)", agent_id, d);
    }

    // Log the reflection as an episode
    let _ = episodic::insert_episode(&db, &Episode {
        id: uuid::Uuid::new_v4().to_string(),
        agent_id: agent_id.into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        event_type: "reflection".into(),
        summary: format!(
            "Self-reflection: {} knowledge entries extracted{}",
            knowledge_entries.len(),
            if context_update.is_some() { ", context updated" } else { "" }
        ),
        raw_data: Some(content.to_string()),
        task_id: Some(task_id.into()),
        outcome: Some("success".into()),
        tokens_used: 0,
        cost_cents: 0,
    });

    if let Some(handle) = &state.app_handle {
        let _ = handle.emit("activity-refresh", ());
        // Toast: show the user what the creature learned
        let count = knowledge_entries.len();
        if count > 0 {
            let msg = if count == 1 {
                "learned 1 fact from that conversation".to_string()
            } else {
                format!("learned {} facts from that conversation", count)
            };
            let _ = handle.emit("toast", serde_json::json!({"type": "learning", "message": msg}));
        }
    }

    tracing::info!(
        "Reflection for agent {}: {} knowledge entries, context {}",
        agent_id,
        knowledge_entries.len(),
        if context_update.is_some() { "updated" } else { "unchanged" }
    );

    Ok(())
}
