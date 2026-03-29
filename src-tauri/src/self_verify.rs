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
    // Budget check
    {
        let db = state.db.lock().await;
        let budget = state.config.read().await.cost.daily_background_token_budget;
        if !crate::token_usage::has_budget_remaining(&db, agent_id, 100, budget)? {
            tracing::info!("Budget exceeded, skipping self-verify for agent {}", agent_id);
            return Ok(());
        }
    }

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
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        tracing::warn!("Self-verify failed: HTTP {} — {}", status, &error_text[..error_text.len().min(200)]);
        // Log to audit trail
        let db = state.db.lock().await;
        let _ = crate::permissions::audit::log_action(&db, &crate::permissions::audit::AuditEntry {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            action_type: "background_error".into(),
            action_detail: format!("Self-verify failed: HTTP {} — {}", status, &error_text[..error_text.len().min(200)]),
            permission_result: "allowed".into(),
            result: None, duration_ms: None, cost_cents: 0,
            error: Some(format!("HTTP {}", status)),
        });
        return Ok(());
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

        // Store as actionable knowledge so the idle thinker can chain on it
        {
            let db = state.db.lock().await;
            let domain_label = effective_domain.as_deref().unwrap_or("unknown");
            let _ = crate::knowledge::insert_knowledge(
                &db, agent_id,
                &format!("FAILED in {}: {}. Need to investigate why.", domain_label, reason),
                "warning", Some(task_id),
            );

            // Also write to scratchpad so idle thinker sees it immediately
            let _ = crate::context::append_context(
                &db, agent_id,
                &format!("Self-verify: BAD in {}. Reason: {}", domain_label, reason),
            );

            // Set urgency flag — idle thinker will react within 60s
            let urgent_key = format!("urgent_think_{}", agent_id);
            let _ = db.execute(
                "INSERT INTO config_store (key, value) VALUES (?1, '1') ON CONFLICT(key) DO UPDATE SET value = '1'",
                rusqlite::params![urgent_key],
            );
        }
    } else {
        tracing::info!("Self-verification: agent {} rated task {} as GOOD", agent_id, task_id);
        // Reward knowledge that was recently injected — it helped produce a good result
        let db = state.db.lock().await;
        let _ = crate::knowledge::bump_success_for_recent(&db, agent_id);
    }

    Ok(())
}
