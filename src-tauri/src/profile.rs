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
    // Budget check
    {
        let db = state.db.lock().await;
        let budget = state.config.read().await.cost.daily_background_token_budget;
        if !crate::token_usage::has_budget_remaining(&db, agent_id, 200, budget)? {
            tracing::info!("Budget exceeded, skipping profile regen for agent {}", agent_id);
            return Ok(());
        }
    }

    // Gather agent data
    let (agent, knowledge_entries, context, trajectory) = {
        let db = state.db.lock().await;
        let agent = registry::get_agent(&db, agent_id)?.ok_or_else(|| anyhow::anyhow!("agent not found"))?;
        let knowledge_entries = knowledge::list_knowledge(&db, agent_id, 10)?;
        let context = crate::context::get_context(&db, agent_id)?;
        let traj = crate::trajectory::build_trajectory_summary(&db, agent_id);
        (agent, knowledge_entries, context, traj)
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

    let has_existing_profile = !agent.dynamic_profile.is_empty();

    let prompt = if has_existing_profile {
        format!(
            r#"Here is this agent's current profile:
"{}"

New data since last update:
- Total tasks: {} (success rate: {}%)
- Recent knowledge: {}
- Current context: {}
- Growth trajectory: {}

Update the profile. Keep what's still true. Remove what's outdated. Add new observations about strengths, weaknesses, or growth trends. The profile should feel like it's growing, not resetting. Write in third person. Max 5 sentences."#,
            agent.dynamic_profile,
            agent.total_tasks, success_rate,
            knowledge_list,
            if context.is_empty() { "None set" } else { &context },
            trajectory,
        )
    } else {
        format!(
            r#"Generate a brief profile for this agent (3-5 sentences, third person):

Agent name: {}
Total tasks: {}, Success rate: {}%
Tools: {}
Context: {}
Knowledge:
{}

Describe what it's good at, patterns in its work, and any preferences. Be concise."#,
            agent.name, agent.total_tasks, success_rate,
            agent.tools_allowed.join(", "),
            if context.is_empty() { "None set" } else { &context },
            knowledge_list,
        )
    };

    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", provider.api_base_url);

    let response = client.post(&url)
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": provider.default_model,
            "messages": [
                {"role": "system", "content": "You are an AI assistant."},
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
