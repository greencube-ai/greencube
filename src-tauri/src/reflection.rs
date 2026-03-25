use std::sync::Arc;
use tauri::Emitter;
use crate::knowledge;
use crate::memory::episodic;
use crate::memory::Episode;
use crate::providers::Provider;
use crate::state::AppState;

const REFLECTION_PROMPT: &str = r#"You just completed a task. Review what happened in the conversation above.

Answer these questions briefly. Use EXACTLY the format shown:
1. What key facts did you learn? (format: [fact] statement)
2. What should you remember for next time? (format: [preference] statement)
3. Were there any mistakes or dead ends? (format: [warning] statement)
4. Update your working context if needed: (format: [context] new content for your scratchpad)

Only include lines that are genuinely useful. If nothing notable was learned, respond with just: NONE"#;

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
    // Build a condensed summary of the conversation (limit to avoid huge prompts)
    let mut summary_messages: Vec<serde_json::Value> = messages.iter()
        .filter(|m| {
            let role = m["role"].as_str().unwrap_or("");
            role == "user" || role == "assistant"
        })
        .take(10) // Max 10 messages to keep reflection prompt reasonable
        .cloned()
        .collect();

    // Inject commandments at the start
    crate::commandments::inject_commandments(&mut summary_messages);

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
        let error_text = response.text().await.unwrap_or_default();
        anyhow::bail!("Reflection LLM call failed: {}", error_text);
    }

    let body: serde_json::Value = response.json().await?;
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("NONE");

    if content.trim() == "NONE" {
        return Ok(());
    }

    // Parse the reflection response (lenient parser)
    let (knowledge_entries, context_update) = knowledge::parse_reflection_response(content);

    let db = state.db.lock().await;

    // Store knowledge entries
    for (category, entry_content) in &knowledge_entries {
        let _ = knowledge::insert_knowledge(&db, agent_id, entry_content, category, Some(task_id));
    }

    // Update working context if provided
    if let Some(ctx) = &context_update {
        let _ = crate::context::append_context(&db, agent_id, ctx);
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
    }

    tracing::info!(
        "Reflection for agent {}: {} knowledge entries, context {}",
        agent_id,
        knowledge_entries.len(),
        if context_update.is_some() { "updated" } else { "unchanged" }
    );

    Ok(())
}
