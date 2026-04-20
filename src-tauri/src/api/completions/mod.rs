use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::StreamExt;
use std::sync::Arc;
use tauri::Emitter;

use crate::identity::registry;
use crate::memory::episodic;
use crate::memory::Episode;
use crate::permissions;
use crate::permissions::audit;
use crate::permissions::audit::AuditEntry;
use crate::state::AppState;
use crate::task_outcome::TaskOutcome;
use crate::competence;

mod delegation;
mod helpers;
mod injection;
mod post_task;
mod streaming;
mod tool_defs;
mod tools;
use delegation::try_self_delegate;
use helpers::{emit_refresh, emit_status, error_response, redact_secrets};
use injection::{
    inject_competence_warning, inject_habitat_knowledge, inject_keyword_knowledge,
    inject_preferences_and_corrections, inject_profile_goals_context, inject_relationship,
    inject_tools_and_hint,
};
use post_task::{finish_task, run_post_task};
use streaming::stream_llm_response;
use tools::execute_tool_call;

const MAX_TOOL_ITERATIONS: usize = 10;

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut body): Json<serde_json::Value>,
) -> Response {
    let wants_stream = body["stream"].as_bool().unwrap_or(false);
    let has_tools = body.get("tools").and_then(|t| t.as_array()).map_or(false, |a| !a.is_empty());

    // 1. RECEIVE REQUEST — extract agent_id
    let agent_id = headers
        .get("x-agent-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // 2. RESOLVE AGENT
    let agent = {
        let db = state.db.lock().await;
        if let Some(ref id) = agent_id {
            match registry::get_agent(&db, id) {
                Ok(Some(a)) => a,
                Ok(None) => return error_response(StatusCode::NOT_FOUND, &format!("agent not found: {}", id)),
                Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
            }
        } else {
            // No agent ID specified — use first available agent, or create "default"
            match registry::list_agents(&db) {
                Ok(agents) if !agents.is_empty() => agents.into_iter().next().expect("checked non-empty"),
                _ => {
                    match registry::get_agent_by_name(&db, "default") {
                        Ok(Some(a)) => a,
                        Ok(None) => {
                            let tools = vec!["shell".into(), "read_file".into(), "write_file".into(), "http_get".into()];
                            match registry::create_agent(&db, "default", "You are a helpful assistant.", &tools) {
                                Ok(a) => a,
                                Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("failed to create default agent: {}", e)),
                            }
                        }
                        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
                    }
                }
            }
        }
    };

    // 2a. SPENDING CAP CHECK
    if agent.max_spend_cents > 0 && agent.total_spend_cents >= agent.max_spend_cents {
        return error_response(
            StatusCode::FORBIDDEN,
            &format!(
                "Agent '{}' has reached its spending cap ({} cents spent of {} cents allowed). Increase the cap in Settings.",
                agent.name, agent.total_spend_cents, agent.max_spend_cents
            ),
        );
    }

    // 2b. SELF-DELEGATION: If agent is weak in the task's domain and has a specialist, reroute
    if let Some(r) = try_self_delegate(&state, &agent, &body).await { return r; }

    // 2c. COMPETENCE WARNING: If agent is weak in detected domain but no specialist, warn
    inject_competence_warning(&state, &agent, &mut body).await;

    // 3. LOG TASK START
    let task_id = uuid::Uuid::new_v4().to_string();
    let task_start = std::time::Instant::now();
    let mut outcome = TaskOutcome::new();
    let user_message_summary = body["messages"]
        .as_array()
        .and_then(|msgs| msgs.iter().rev().find(|m| m["role"] == "user"))
        .and_then(|m| m["content"].as_str())
        .unwrap_or("")
        .chars()
        .take(200)
        .collect::<String>();

    {
        let db = state.db.lock().await;
        let _ = registry::update_agent_status(&db, &agent.id, "active");
        let _ = episodic::insert_episode(&db, &Episode {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent.id.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
            event_type: "task_start".into(),
            summary: format!("Task started: {}", user_message_summary),
            raw_data: None,
            task_id: Some(task_id.clone()),
            outcome: None,
            tokens_used: 0,
            cost_cents: 0,
        });
        // Also log to audit_log so it shows in Activity Feed
        let _ = audit::log_action(&db, &AuditEntry {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent.id.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
            action_type: "task_start".into(),
            action_detail: redact_secrets(&format!("Task started: {}", user_message_summary)),
            permission_result: "allowed".into(),
            result: None, duration_ms: None, cost_cents: 0, error: None,
        });
    }
    emit_status(&state, &agent.id, "active");
    emit_refresh(&state);

    // Track relationship with user (use x-user-id header or "default_user")
    let user_id = headers.get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("default_user")
        .to_string();
    {
        let db = state.db.lock().await;
        let _ = crate::relationships::record_interaction(&db, &agent.id, &user_id);
    }

    // Inject relationship context if enough interactions
    inject_relationship(&state, &agent, &user_id, &mut body).await;

    // 3b. INJECT LEARNED PREFERENCES + MISTAKES TO AVOID
    inject_preferences_and_corrections(&state, &agent, &mut body).await;

    // 3c. INJECT WORKING CONTEXT (scratchpad) + DYNAMIC PROFILE
    inject_profile_goals_context(&state, &agent, &mut body).await;

    // 4. INJECT KNOWLEDGE — try semantic search via Ollama, fall back to keyword
    inject_keyword_knowledge(&state, &agent, &mut body).await;

    // 4b. CROSS-AGENT LEARNING: inject relevant knowledge from other agents in the habitat
    inject_habitat_knowledge(&state, &agent, &mut body).await;

    // 5. INJECT TOOL DEFINITIONS + tool-usage hint
    inject_tools_and_hint(&state, &agent, &mut body).await;

    // Look up agent's provider (or fall back to default)
    let provider = {
        let db = state.db.lock().await;
        match crate::providers::get_provider_for_agent(&db, &agent) {
            Ok(p) => p,
            Err(e) => return error_response(StatusCode::SERVICE_UNAVAILABLE, &e.to_string()),
        }
    };
    let client = reqwest::Client::new();
    let llm_url = format!("{}/chat/completions", provider.api_base_url);

    // Re-check: do we now have tools after injection?
    let has_tools = body.get("tools").and_then(|t| t.as_array()).map_or(false, |a| !a.is_empty());

    // STREAMING PATH: If client wants streaming AND no tools, stream directly from the first call
    if wants_stream && !has_tools {
        body["stream"] = serde_json::Value::Bool(true);
        return stream_llm_response(state.clone(), &client, &llm_url, &provider, &body, &agent.id, &task_id).await;
    }

    // NON-STREAMING / TOOL-CALL PATH
    body["stream"] = serde_json::Value::Bool(false);
    let mut total_tokens = 0i64;
    let mut total_cost = 0i64;
    let mut iteration = 0;
    // Sticky flag: set true if ANY tool call returns an error across all loop
    // iterations. Never resets to false — per the success definition, a task
    // that had any tool error is not successful even if the LLM recovered.
    let mut has_tool_error = false;

    loop {
        iteration += 1;
        outcome.record_llm_round();
        if iteration > MAX_TOOL_ITERATIONS {
            let db = state.db.lock().await;
            let _ = registry::update_agent_status(&db, &agent.id, "idle");
            emit_status(&state, &agent.id, "idle");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "tool call loop exceeded 10 iterations. Possible infinite loop.");
        }

        // Forward to LLM (non-streaming)
        let llm_response = match client.post(&llm_url)
            .header("Authorization", format!("Bearer {}", provider.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let db = state.db.lock().await;
                let _ = registry::update_agent_status(&db, &agent.id, "idle");
                let _ = episodic::insert_episode(&db, &Episode {
                    id: uuid::Uuid::new_v4().to_string(), agent_id: agent.id.clone(),
                    created_at: chrono::Utc::now().to_rfc3339(), event_type: "error".into(),
                    summary: format!("LLM API unreachable: {}", e), raw_data: None,
                    task_id: Some(task_id.clone()), outcome: Some("failure".into()),
                    tokens_used: 0, cost_cents: 0,
                });
                emit_status(&state, &agent.id, "idle");
                emit_refresh(&state);
                return error_response(StatusCode::BAD_GATEWAY, &format!("could not reach LLM API at {}. Check your API key and network.", llm_url));
            }
        };

        let llm_status = llm_response.status();
        if !llm_status.is_success() {
            let db = state.db.lock().await;
            let _ = registry::update_agent_status(&db, &agent.id, "idle");
            emit_status(&state, &agent.id, "idle");
            let error_text = llm_response.text().await.unwrap_or_default();
            if llm_status == reqwest::StatusCode::UNAUTHORIZED {
                if let Some(handle) = &state.app_handle {
                    let _ = handle.emit("toast", serde_json::json!({"type": "error", "message": "Invalid API key. Check your LLM API key in Settings."}));
                }
                return error_response(StatusCode::UNAUTHORIZED, "Invalid API key. Update your key in Settings.");
            }

            // 429 Rate Limited — pass through error AND queue retry internally
            if llm_status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry_after: u64 = 60; // Default retry in 60 seconds
                let messages_json = serde_json::to_string(&body["messages"]).unwrap_or_default();
                let _ = crate::task_queue::queue_rate_limited(
                    &db, &agent.id, &messages_json, agent.provider_id.as_deref(), retry_after,
                );
                let _ = registry::update_agent_status(&db, &agent.id, "queued");
                emit_status(&state, &agent.id, "queued");
                let _ = crate::notifications::create_notification(
                    &db, &agent.id,
                    &format!("Rate limited. I'll retry automatically in {} seconds.", retry_after),
                    "alert", "task_queue"
                );
                if let Some(handle) = &state.app_handle {
                    let _ = handle.emit("notification-new", serde_json::json!({"agent_id": &agent.id}));
                }
                emit_refresh(&state);
                // Pass through the 429 to the client as-is
                return error_response(StatusCode::TOO_MANY_REQUESTS, &format!("Rate limited. GreenCube will retry automatically in {} seconds.", retry_after));
            }

            return error_response(StatusCode::BAD_GATEWAY, &format!("LLM API returned {}. {}", llm_status, error_text));
        }

        let response_body: serde_json::Value = match llm_response.json().await {
            Ok(v) => v,
            Err(e) => {
                let db = state.db.lock().await;
                let _ = registry::update_agent_status(&db, &agent.id, "idle");
                emit_status(&state, &agent.id, "idle");
                return error_response(StatusCode::BAD_GATEWAY, &format!("Failed to parse LLM response: {}", e));
            }
        };

        // Track tokens
        if let Some(usage) = response_body.get("usage") {
            let tokens = usage["total_tokens"].as_i64().unwrap_or(0);
            total_tokens += tokens;
            total_cost += tokens / 100;
        }

        // Log LLM response episode + audit entry
        let response_content = response_body["choices"][0]["message"]["content"].as_str().unwrap_or("").chars().take(200).collect::<String>();
        let response_summary = if response_content.is_empty() {
            format!("LLM responded with tool calls (iteration {})", iteration)
        } else {
            format!("LLM: {}", response_content)
        };
        {
            let db = state.db.lock().await;
            let _ = episodic::insert_episode(&db, &Episode {
                id: uuid::Uuid::new_v4().to_string(), agent_id: agent.id.clone(),
                created_at: chrono::Utc::now().to_rfc3339(), event_type: "llm_response".into(),
                summary: response_summary.clone(),
                raw_data: Some(serde_json::to_string(&response_body).unwrap_or_default()),
                task_id: Some(task_id.clone()), outcome: Some("success".into()),
                tokens_used: total_tokens, cost_cents: total_cost,
            });
            let _ = audit::log_action(&db, &AuditEntry {
                id: uuid::Uuid::new_v4().to_string(), agent_id: agent.id.clone(),
                created_at: chrono::Utc::now().to_rfc3339(), action_type: "llm_response".into(),
                action_detail: redact_secrets(&response_summary),
                permission_result: "allowed".into(),
                result: None, duration_ms: None, cost_cents: total_cost, error: None,
            });
        }
        emit_refresh(&state);

        // MISTAKE PREVENTION: check response against known corrections before returning
        // Only on the first non-tool response (no retry loop), and only once per task
        let assistant_content = response_body["choices"][0]["message"]["content"].as_str().unwrap_or("");
        if !assistant_content.is_empty() && iteration == 1 {
            // Words that appear in every correction but carry no meaning for matching
            const CORRECTION_STOP: &[&str] = &[
                "correction", "user", "rejected", "response", "about", "repeat",
                "approach", "approved", "output", "avoid", "future", "task",
                "disapproved", "praised", "general", "unknown",
            ];

            let matching_corrections: Vec<String> = {
                let db = state.db.lock().await;
                let all_knowledge = crate::knowledge::list_knowledge(&db, &agent.id, 100).unwrap_or_default();
                all_knowledge.iter()
                    .filter(|k| k.category == "correction")
                    .filter(|k| {
                        // Extract meaningful words only (skip boilerplate and short words)
                        let correction_words: Vec<String> = k.content.split_whitespace()
                            .map(|w| w.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>())
                            .filter(|w| w.len() > 4)
                            .filter(|w| !CORRECTION_STOP.contains(&w.as_str()))
                            .collect();
                        let response_lower = assistant_content.to_lowercase();
                        let matches = correction_words.iter().filter(|w| response_lower.contains(w.as_str())).count();
                        // Require 3+ meaningful words AND 75% match
                        correction_words.len() >= 3 && matches as f64 >= correction_words.len() as f64 * 0.75
                    })
                    .take(2)
                    .map(|k| k.content.clone())
                    .collect()
            };

            if !matching_corrections.is_empty() {
                tracing::info!("Mistake prevention: detected {} matching corrections for agent {}", matching_corrections.len(), agent.id);

                // Inject warning and retry ONCE
                let warning = format!(
                    "\n\nWARNING: A similar response previously received negative feedback. Issues were:\n{}\nRevise your answer to avoid these mistakes.",
                    matching_corrections.iter().map(|c| format!("- {}", c)).collect::<Vec<_>>().join("\n")
                );

                if let Some(messages) = body["messages"].as_array_mut() {
                    if let Some(sys) = messages.iter_mut().find(|m| m["role"] == "system") {
                        if let Some(c) = sys["content"].as_str() {
                            sys["content"] = serde_json::Value::String(format!("{}{}", c, warning));
                        }
                    }
                }

                // Log the prevention + increment counter
                {
                    let db = state.db.lock().await;
                    crate::api::brain::increment_counter(&db, &agent.id, "mistakes_prevented");
                    let _ = audit::log_action(&db, &AuditEntry {
                        id: uuid::Uuid::new_v4().to_string(), agent_id: agent.id.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(), action_type: "mistake_prevented".into(),
                        action_detail: format!("Detected {} known mistake patterns in response. Retrying with correction.", matching_corrections.len()),
                        permission_result: "allowed".into(),
                        result: None, duration_ms: None, cost_cents: 0, error: None,
                    });
                }
                emit_refresh(&state);

                // Skip this response and let the loop retry with the injected warning
                // (iteration will increment, so it won't trigger again)
                continue;
            }
        }

        // CHECK FOR TOOL CALLS
        let assistant_msg = &response_body["choices"][0]["message"];
        let tool_calls = assistant_msg.get("tool_calls").and_then(|tc| tc.as_array());

        if let Some(tool_calls) = tool_calls {
            if tool_calls.is_empty() {
                // has_tool_error may be true from a prior loop iteration
                let msgs = body["messages"].as_array().map(|a| a.to_vec());
                outcome.finalize(task_start.elapsed(), total_cost);
                return finish_task(state.clone(), &agent.id, &task_id, !has_tool_error, total_cost, response_body, Some(&provider), msgs.as_deref(), outcome).await;
            }

            let mut tool_results = Vec::new();
            for tc in tool_calls {
                let tc_id = tc["id"].as_str().unwrap_or("unknown");
                let func_name = tc["function"]["name"].as_str().unwrap_or("unknown");
                let func_args: serde_json::Value = tc["function"]["arguments"].as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(serde_json::Value::Object(Default::default()));

                {
                    let db = state.db.lock().await;
                    let _ = audit::log_action(&db, &AuditEntry {
                        id: uuid::Uuid::new_v4().to_string(), agent_id: agent.id.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(), action_type: "tool_call".into(),
                        action_detail: redact_secrets(&serde_json::json!({"tool": func_name, "arguments": func_args}).to_string()),
                        permission_result: if permissions::check_tool_permission(&agent, func_name) { "allowed" } else { "denied" }.into(),
                        result: None, duration_ms: None, cost_cents: 0, error: None,
                    });
                }
                emit_refresh(&state);

                let tool_result = if !permissions::check_tool_permission(&agent, func_name) {
                    format!("Permission denied: agent does not have access to tool '{}'", func_name)
                } else {
                    execute_tool_call(&state, &agent.id, func_name, &func_args).await
                };

                if tool_result.starts_with("Error:") || tool_result.starts_with("Permission denied:") {
                    has_tool_error = true;
                    outcome.record_tool_call(true);
                } else {
                    outcome.record_tool_call(false);
                }

                tool_results.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tc_id,
                    "content": tool_result,
                }));
            }

            if let Some(messages) = body["messages"].as_array_mut() {
                messages.push(assistant_msg.clone());
                messages.extend(tool_results);
            }
        } else {
            // No tool_calls in this response — LLM gave a final text answer.
            // has_tool_error may be true from a prior loop iteration.
            let msgs = body["messages"].as_array().map(|a| a.to_vec());
            outcome.finalize(task_start.elapsed(), total_cost);
            return finish_task(state.clone(), &agent.id, &task_id, !has_tool_error, total_cost, response_body, Some(&provider), msgs.as_deref(), outcome).await;
        }
    }
}

#[cfg(test)]
#[path = "integration_test.rs"]
mod integration_test;
