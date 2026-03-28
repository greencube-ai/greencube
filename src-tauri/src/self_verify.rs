use std::sync::Arc;
use crate::competence;
use crate::providers::Provider;
use crate::state::AppState;

const VERIFY_PROMPT: &str = r#"Look at the conversation above. Did your response actually solve what the user asked? Was it accurate and complete?

Rate yourself honestly. Use EXACTLY one of these on its own line:
[quality] good
[quality] bad — one sentence explaining why

Be honest. If you're not sure, say bad."#;

/// Run self-verification after a task completes. Spawns as background task.
pub fn spawn_verification(
    state: Arc<AppState>,
    agent_id: String,
    provider: Provider,
    messages: Vec<serde_json::Value>,
    task_id: String,
    domain: Option<String>,
) {
    tokio::spawn(async move {
        if let Err(e) = run_verification(&state, &agent_id, &provider, &messages, &task_id, domain.as_deref()).await {
            tracing::warn!("Self-verification failed for agent {}: {}", agent_id, e);
        }
    });
}

async fn run_verification(
    state: &AppState,
    agent_id: &str,
    provider: &Provider,
    messages: &[serde_json::Value],
    task_id: &str,
    domain: Option<&str>,
) -> anyhow::Result<()> {
    // Build condensed conversation for verification
    let mut verify_messages: Vec<serde_json::Value> = messages.iter()
        .filter(|m| {
            let role = m["role"].as_str().unwrap_or("");
            role == "user" || role == "assistant"
        })
        .take(6)
        .cloned()
        .collect();

    verify_messages.push(serde_json::json!({
        "role": "user",
        "content": VERIFY_PROMPT
    }));

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/chat/completions", provider.api_base_url))
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .json(&serde_json::json!({
            "model": provider.default_model,
            "messages": verify_messages,
            "max_tokens": 100,
            "temperature": 0.1,
        }))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?;

    if !response.status().is_success() {
        return Ok(()); // Don't fail the task if verification fails
    }

    let body: serde_json::Value = response.json().await?;
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("good");

    // Parse [quality] tag — find it anywhere in the response
    let quality_good = if let Some(idx) = content.find("[quality]") {
        let after = content[idx + 9..].trim();
        after.starts_with("good")
    } else {
        true // Default to good if no tag found
    };

    if !quality_good {
        // Extract reason
        let reason = if let Some(idx) = content.find("[quality] bad") {
            let after = content[idx + 13..].trim();
            let reason = after.trim_start_matches("—").trim_start_matches("-").trim();
            if reason.is_empty() { "self-rated as bad" } else { reason }
        } else {
            "self-rated as bad"
        };

        tracing::info!("Self-verification: agent {} rated task {} as BAD: {}", agent_id, task_id, reason);

        // Update competence with failure — use provided domain or look up most recent
        let effective_domain = match domain {
            Some(d) => Some(d.to_string()),
            None => {
                let db = state.db.lock().await;
                competence::get_most_recent_domain(&db, agent_id).ok().flatten()
            }
        };
        if let Some(ref d) = effective_domain {
            let db = state.db.lock().await;
            let _ = competence::update_competence(&db, agent_id, d, false, None);
            tracing::info!("Competence updated: agent {} domain '{}' (FAILURE from self-verify)", agent_id, d);
        }

        // Store as knowledge: agent knows it failed
        {
            let db = state.db.lock().await;
            let _ = crate::knowledge::insert_knowledge(
                &db, agent_id,
                &format!("Self-assessment: produced poor quality output. Reason: {}", reason),
                "warning", Some(task_id),
            );
        }
    } else {
        tracing::info!("Self-verification: agent {} rated task {} as GOOD", agent_id, task_id);
    }

    Ok(())
}
