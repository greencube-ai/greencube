use std::sync::Arc;
use crate::identity::registry;
use crate::knowledge;
use crate::providers::Provider;
use crate::state::AppState;

const PROFILE_INTERVAL: i64 = 5; // Regenerate every 5 tasks

/// Check if profile should be regenerated (every N tasks) and spawn if so.
pub fn maybe_regenerate(state: Arc<AppState>, agent_id: String, provider: Provider, total_tasks: i64) {
    if total_tasks > 0 && total_tasks % PROFILE_INTERVAL == 0 {
        tokio::spawn(async move {
            if let Err(e) = regenerate_profile(&state, &agent_id, &provider).await {
                tracing::warn!("Profile regeneration failed for {}: {}", agent_id, e);
            }
        });
    }
}

async fn regenerate_profile(state: &AppState, agent_id: &str, provider: &Provider) -> anyhow::Result<()> {
    // Gather agent data
    let (agent, knowledge_entries, context) = {
        let db = state.db.lock().await;
        let agent = registry::get_agent(&db, agent_id)?.ok_or_else(|| anyhow::anyhow!("agent not found"))?;
        let knowledge_entries = knowledge::list_knowledge(&db, agent_id, 10)?;
        let context = crate::context::get_context(&db, agent_id)?;
        (agent, knowledge_entries, context)
    };

    let success_rate = if agent.total_tasks > 0 {
        (agent.successful_tasks as f64 / agent.total_tasks as f64 * 100.0) as i64
    } else { 0 };

    let knowledge_list = if knowledge_entries.is_empty() {
        "No knowledge entries yet.".to_string()
    } else {
        knowledge_entries.iter()
            .map(|k| format!("- [{}] {}", k.category, k.content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let prompt = format!(
        r#"Based on this agent's history, generate a brief profile (3-5 sentences max, 500 characters max):

Agent name: {}
Total tasks: {}, Success rate: {}%
Tools available: {}
Current working context: {}
Knowledge base:
{}

Describe: what this agent is good at, typical work patterns, and any preferences learned. If limited data is available, generate a brief profile based on what's known. Be concise."#,
        agent.name, agent.total_tasks, success_rate,
        agent.tools_allowed.join(", "),
        if context.is_empty() { "None set" } else { &context },
        knowledge_list,
    );

    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", provider.api_base_url);

    let response = client.post(&url)
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": provider.default_model,
            "messages": [
                {"role": "system", "content": crate::commandments::AGENT_COMMANDMENTS},
                {"role": "user", "content": prompt}
            ],
            "max_tokens": 200,
            "temperature": 0.3,
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Profile generation LLM call failed");
    }

    let body: serde_json::Value = response.json().await?;
    let profile = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(500) // Hard limit 500 chars
        .collect::<String>();

    if !profile.is_empty() {
        let db = state.db.lock().await;
        registry::update_agent_dynamic_profile(&db, agent_id, &profile)?;
        tracing::info!("Updated dynamic profile for agent {}", agent_id);
    }

    Ok(())
}
