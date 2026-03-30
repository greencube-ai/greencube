use std::sync::Arc;
use tauri::Emitter;
use crate::competence;
use crate::providers::Provider;
use crate::state::AppState;

const VERIFY_PROMPT: &str = r#"You are a strict code reviewer. Grade the assistant's response above on a 1-5 scale.

Be HARSH. Most responses have flaws. A 5 is exceptional — almost nothing deserves it.

[score] N — one sentence justification

Scoring:
1 = wrong, harmful, or completely off-topic
2 = partially correct but missing key details, or contains errors
3 = acceptable but could be better (missing edge cases, verbose, not quite what was asked)
4 = good, addresses the question well with minor room for improvement
5 = excellent, complete, accurate, concise — nothing to improve

Most responses are a 3. Be critical. If unsure between two scores, pick the lower one."#;

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
    // Budget check — read config before db lock to avoid deadlock
    let budget = state.config.read().await.cost.daily_background_token_budget;
    {
        let db = state.db.lock().await;
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

    // Record token usage for budget tracking
    let tokens_used = body["usage"]["total_tokens"].as_i64().unwrap_or(100);
    {
        let db = state.db.lock().await;
        let _ = crate::token_usage::record_usage(&db, agent_id, "self_verify", tokens_used);
    }

    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("[score] 3");

    // Parse [score] N — extract the number 1-5
    let score: i32 = if let Some(idx) = content.find("[score]") {
        let after = content[idx + 7..].trim();
        after.chars().next().and_then(|c| c.to_digit(10)).map(|d| d as i32).unwrap_or(3)
    } else {
        // Try to find just a standalone digit at the start
        content.trim().chars().next().and_then(|c| c.to_digit(10)).map(|d| d as i32).unwrap_or(3)
    };
    let score = score.clamp(1, 5);

    // Extract reason (everything after the number)
    let reason = if let Some(idx) = content.find("[score]") {
        let after = content[idx + 7..].trim();
        let after = after.trim_start_matches(|c: char| c.is_numeric() || c == ' ');
        let reason = after.trim_start_matches("—").trim_start_matches("-").trim();
        if reason.is_empty() { "no details" } else { reason }
    } else { "no details" };

    tracing::info!("Self-verification: agent {} task {} scored {}/5: {}", agent_id, task_id, score, &reason[..reason.len().min(80)]);

    // Get domain
    let effective_domain = match domain {
        Some(d) => Some(d.to_string()),
        None => {
            let db = state.db.lock().await;
            competence::get_most_recent_domain(&db, agent_id).ok().flatten()
        }
    };

    // Score 1-2: failure (competence drops, warning stored, urgency flag)
    // Score 3: partial (competence marked as failure — this is the key change, 3 is not "good")
    // Score 4-5: success (competence improves, knowledge rewarded)
    let is_success = score >= 4;

    if let Some(ref d) = effective_domain {
        let db = state.db.lock().await;
        let _ = competence::update_competence(&db, agent_id, d, is_success, None);
    }

    if score <= 2 {
        // Bad: store warning, urgency flag, toast
        let db = state.db.lock().await;
        let domain_label = effective_domain.as_deref().unwrap_or("unknown");
        let _ = crate::knowledge::insert_knowledge(
            &db, agent_id,
            &format!("FAILED in {}: {}. Need to investigate why.", domain_label, reason),
            "warning", Some(task_id),
        );
        let _ = crate::context::append_context(
            &db, agent_id,
            &format!("Self-verify: {}/5 in {}. {}", score, domain_label, reason),
        );
        let urgent_key = format!("urgent_think_{}", agent_id);
        let _ = db.execute(
            "INSERT INTO config_store (key, value) VALUES (?1, '1') ON CONFLICT(key) DO UPDATE SET value = '1'",
            rusqlite::params![urgent_key],
        );
        if let Some(handle) = &state.app_handle {
            let _ = handle.emit("toast", serde_json::json!({"type": "verify_bad", "message": format!("self-check: {}/5 — {}", score, &reason[..reason.len().min(50)])}));
        }
    } else if score == 3 {
        // Mediocre: competence already marked as failure above, toast but no urgency
        if let Some(handle) = &state.app_handle {
            let _ = handle.emit("toast", serde_json::json!({"type": "verify_bad", "message": format!("self-check: 3/5 — {}", &reason[..reason.len().min(50)])}));
        }
    } else {
        // Good (4-5): reward knowledge, toast
        let db = state.db.lock().await;
        let _ = crate::knowledge::bump_success_for_recent(&db, agent_id);
        if let Some(handle) = &state.app_handle {
            let msg = if score == 5 { "self-check: 5/5 — excellent" } else { "self-check: 4/5 — good" };
            let _ = handle.emit("toast", serde_json::json!({"type": "verify_good", "message": msg}));
        }
    }

    Ok(())
}
