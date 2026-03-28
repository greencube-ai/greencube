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
use crate::sandbox::docker as sandbox_docker;
use crate::sandbox::SandboxOptions;
use crate::state::AppState;

const MAX_TOOL_ITERATIONS: usize = 10;

/// Helper to emit refresh signals to the frontend
fn emit_refresh(state: &AppState) {
    if let Some(handle) = &state.app_handle {
        let _ = handle.emit("activity-refresh", ());
    }
}

fn emit_status(state: &AppState, agent_id: &str, status: &str) {
    if let Some(handle) = &state.app_handle {
        let _ = handle.emit(
            "agent-status-change",
            serde_json::json!({"id": agent_id, "status": status}),
        );
    }
}

fn error_response(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}

/// SECURITY: Redact sensitive patterns from strings before logging to audit/episodes.
/// Catches Bearer tokens, API keys, passwords, and Authorization headers.
fn redact_secrets(s: &str) -> String {
    use std::sync::LazyLock;
    static RE: LazyLock<Vec<(regex::Regex, &'static str)>> = LazyLock::new(|| vec![
        (regex::Regex::new(r"(?i)(bearer\s+)[a-zA-Z0-9_\-\.]+").unwrap(), "${1}[REDACTED]"),
        (regex::Regex::new(r"(?i)(api[_-]?key[=:\s]+)[a-zA-Z0-9_\-\.]+").unwrap(), "${1}[REDACTED]"),
        (regex::Regex::new(r"(?i)(authorization[=:\s]+)[^\s,\}]+").unwrap(), "${1}[REDACTED]"),
        (regex::Regex::new(r"sk-[a-zA-Z0-9]{20,}").unwrap(), "[REDACTED_KEY]"),
    ]);
    let mut result = s.to_string();
    for (re, replacement) in RE.iter() {
        result = re.replace_all(&result, *replacement).to_string();
    }
    result
}

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
    {
        let user_msg_lower = body["messages"]
            .as_array()
            .and_then(|msgs| msgs.iter().rev().find(|m| m["role"] == "user"))
            .and_then(|m| m["content"].as_str())
            .unwrap_or("")
            .to_lowercase();

        let db = state.db.lock().await;
        let competence_map = crate::competence::get_competence_map(&db, &agent.id).unwrap_or_default();
        let children = crate::spawn::get_children(&db, &agent.id);

        // Find a domain where: agent has <50% competence, 5+ tasks, AND a specialist child
        let mut delegation_target: Option<(String, String, f64)> = None; // (child_id, domain, confidence)
        for entry in &competence_map {
            if entry.confidence < 0.50 && entry.task_count >= 5 {
                // Check if a specialist exists for this domain
                if let Some(child) = children.iter().find(|c| c.2 == entry.domain) {
                    // Check if the user's message is related to this domain
                    if user_msg_lower.contains(&entry.domain) {
                        delegation_target = Some((child.0.clone(), entry.domain.clone(), entry.confidence));
                        break;
                    }
                }
            }
        }
        drop(db);

        if let Some((child_id, domain, confidence)) = delegation_target {
            tracing::info!(
                "Self-delegation: {} is weak in {} ({:.0}%), routing to specialist {}",
                agent.name, domain, confidence * 100.0, child_id
            );

            // Reroute: change the X-Agent-Id to the specialist and re-process
            let db = state.db.lock().await;
            if let Ok(Some(specialist)) = registry::get_agent(&db, &child_id) {
                drop(db);

                // Log the delegation
                {
                    let db = state.db.lock().await;
                    let _ = audit::log_action(&db, &AuditEntry {
                        id: uuid::Uuid::new_v4().to_string(),
                        agent_id: agent.id.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        action_type: "delegation".into(),
                        action_detail: format!("Task delegated to {} (competence: {:.0}% in {})", specialist.name, confidence * 100.0, domain),
                        permission_result: "allowed".into(),
                        result: None, duration_ms: None, cost_cents: 0, error: None,
                    });
                }
                emit_refresh(&state);

                // Forward the request to the specialist by sending it as a message
                let user_content = body["messages"]
                    .as_array()
                    .and_then(|msgs| msgs.iter().rev().find(|m| m["role"] == "user"))
                    .and_then(|m| m["content"].as_str())
                    .unwrap_or("")
                    .to_string();

                match crate::agent_messages::send_message(&state, &agent.id, &specialist.name, &user_content, 0).await {
                    Ok(response) => {
                        let resp_body = serde_json::json!({
                            "id": format!("chatcmpl-greencube-delegated-{}", uuid::Uuid::new_v4()),
                            "object": "chat.completion",
                            "created": chrono::Utc::now().timestamp(),
                            "model": "delegated",
                            "choices": [{
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": format!("[Handled by {} — {} specialist]\n\n{}", specialist.name, domain, response)
                                },
                                "finish_reason": "stop"
                            }],
                            "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
                        });
                        return Json(resp_body).into_response();
                    }
                    Err(e) => {
                        tracing::warn!("Delegation failed, proceeding with parent: {}", e);
                        // Fall through to normal processing
                    }
                }
            }
        }
    }

    // 2c. COMPETENCE WARNING: If agent is weak in detected domain but no specialist, warn
    {
        let user_msg_lower = body["messages"]
            .as_array()
            .and_then(|msgs| msgs.iter().rev().find(|m| m["role"] == "user"))
            .and_then(|m| m["content"].as_str())
            .unwrap_or("")
            .to_lowercase();

        let db = state.db.lock().await;
        let competence_map = crate::competence::get_competence_map(&db, &agent.id).unwrap_or_default();

        for entry in &competence_map {
            if entry.task_count >= 5 && entry.confidence < 0.50 && user_msg_lower.contains(&entry.domain) {
                let children = crate::spawn::get_children(&db, &agent.id);
                let has_specialist = children.iter().any(|c| c.2 == entry.domain);

                if !has_specialist {
                    // Inject warning into system prompt so the agent is careful
                    if let Some(messages) = body["messages"].as_array_mut() {
                        let warning = format!(
                            "\n\nWARNING: You have a {:.0}% success rate in {} ({} tasks). Your response may need extra verification. Flag any uncertainty.",
                            entry.confidence * 100.0, entry.domain, entry.task_count
                        );
                        if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
                            if let Some(content) = system_msg["content"].as_str() {
                                system_msg["content"] = serde_json::Value::String(format!("{}{}", content, warning));
                            }
                        }
                    }
                }
                break;
            }
        }
    }

    // 3. LOG TASK START
    let task_id = uuid::Uuid::new_v4().to_string();
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

    // 3a. INJECT COMMANDMENTS (always first, non-negotiable)
    if let Some(messages) = body["messages"].as_array_mut() {
        crate::commandments::inject_commandments(messages);
    }

    // 3b. INJECT LEARNED PREFERENCES
    // The agent's behavior changes based on what it learned about the user.
    {
        let db = state.db.lock().await;
        let all_knowledge = crate::knowledge::list_knowledge(&db, &agent.id, 100).unwrap_or_default();
        let preferences: Vec<String> = all_knowledge.iter()
            .filter(|k| k.category == "preference")
            .take(5)
            .map(|k| format!("- {}", k.content))
            .collect();
        if !preferences.is_empty() {
            if let Some(messages) = body["messages"].as_array_mut() {
                let pref_text = format!(
                    "\n\n--- Apply these learned preferences ---\n{}\n--- End preferences ---",
                    preferences.join("\n")
                );
                if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
                    if let Some(content) = system_msg["content"].as_str() {
                        system_msg["content"] = serde_json::Value::String(format!("{}{}", content, pref_text));
                    }
                }
            }
        }
    }

    // 3c. INJECT WORKING CONTEXT (scratchpad) + DYNAMIC PROFILE
    if let Some(messages) = body["messages"].as_array_mut() {
        let mut injections = Vec::new();

        // Dynamic profile (max 500 chars)
        if !agent.dynamic_profile.is_empty() {
            let profile: String = agent.dynamic_profile.chars().take(500).collect();
            injections.push(format!("--- Your profile ---\n{}\n--- End profile ---", profile));
        }

        // Goals + working context
        {
            let db = state.db.lock().await;

            // Active goals
            let active_goals = crate::goals::list_goals(&db, &agent.id, Some("active")).unwrap_or_default();
            if !active_goals.is_empty() {
                let goals_text = active_goals.iter()
                    .enumerate()
                    .map(|(i, g)| format!("{}. {}", i + 1, g.content))
                    .collect::<Vec<_>>().join("\n");
                injections.push(format!("--- Your current goals ---\n{}\n--- End goals ---", goals_text));
            }

            // Working context (max 1000 chars)
            let ctx = crate::context::get_context(&db, &agent.id).unwrap_or_default();
            if !ctx.is_empty() {
                injections.push(format!(
                    "--- Your working context (you can update this with the update_context tool) ---\n{}\n--- End working context ---",
                    ctx.chars().take(1000).collect::<String>()
                ));
            }
        }

        if !injections.is_empty() {
            let injection_text = injections.join("\n\n");
            if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
                if let Some(content) = system_msg["content"].as_str() {
                    system_msg["content"] = serde_json::Value::String(format!("{}\n\n{}", content, injection_text));
                }
            } else {
                messages.insert(0, serde_json::json!({"role": "system", "content": injection_text}));
            }
        }
    }

    // 4. INJECT KNOWLEDGE (replaces raw episode injection)
    let memory_mode = state.config.read().await.llm.memory_mode.clone();
    if memory_mode == "keyword" {
        if let Some(messages) = body["messages"].as_array_mut() {
            if let Some(last_user_msg) = messages.iter().rev().find(|m| m["role"] == "user").and_then(|m| m["content"].as_str()) {
                let db = state.db.lock().await;
                // Try knowledge first (structured), fall back to episodes (raw)
                let knowledge = crate::knowledge::recall_relevant(&db, &agent.id, last_user_msg, 10).unwrap_or_default();
                if !knowledge.is_empty() {
                    let knowledge_text = knowledge.iter()
                        .map(|k| format!("- [{}] {}", k.category, k.content))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let injection = format!("\n\n--- Things you know ---\n{}\n--- End knowledge ---", knowledge_text);
                    if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
                        if let Some(content) = system_msg["content"].as_str() {
                            system_msg["content"] = serde_json::Value::String(format!("{}{}", content, injection));
                        }
                    }
                } else {
                    // Fall back to episode-based recall if no knowledge entries yet
                    if let Ok(memories) = episodic::recall_relevant_episodes(&db, &agent.id, last_user_msg, 5) {
                        inject_memories(messages, &memories);
                    }
                }
            }
        }
    }

    // 4b. CROSS-AGENT LEARNING: inject relevant knowledge from other agents in the habitat
    if let Some(last_user_msg) = body["messages"].as_array()
        .and_then(|msgs| msgs.iter().rev().find(|m| m["role"] == "user"))
        .and_then(|m| m["content"].as_str())
        .map(|s| s.to_string())
    {
        let db = state.db.lock().await;
        let habitat_knowledge = crate::knowledge::recall_habitat_knowledge(&db, &agent.id, &last_user_msg, 3).unwrap_or_default();
        drop(db);

        if !habitat_knowledge.is_empty() {
            let habitat_text = habitat_knowledge.iter()
                .map(|(agent_name, k)| format!("- [from {}] {}", agent_name, k.content))
                .collect::<Vec<_>>()
                .join("\n");

            if let Some(messages) = body["messages"].as_array_mut() {
                let injection = format!("\n\n--- Knowledge from other agents in your habitat ---\n{}\n--- End habitat knowledge ---", habitat_text);
                if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
                    if let Some(content) = system_msg["content"].as_str() {
                        system_msg["content"] = serde_json::Value::String(format!("{}{}", content, injection));
                    }
                }
            }
        }
    }

    // 5. INJECT TOOL DEFINITIONS
    // If the agent has tools but the request doesn't include tool definitions,
    // inject them so the LLM knows what tools are available.
    if !agent.tools_allowed.is_empty() {
        let tool_defs = build_tool_definitions(&agent.tools_allowed);
        if !tool_defs.is_empty() {
            if body.get("tools").and_then(|t| t.as_array()).map_or(true, |a| a.is_empty()) {
                // No tools in request — inject agent's tools
                body["tools"] = serde_json::Value::Array(tool_defs);
            } else {
                // Client sent tools — filter to only those the agent is allowed
                let filtered: Vec<serde_json::Value> = body["tools"].as_array().unwrap().iter()
                    .filter(|t| t["function"]["name"].as_str().map(|name| permissions::check_tool_permission(&agent, name)).unwrap_or(false))
                    .cloned()
                    .collect();
                body["tools"] = serde_json::Value::Array(filtered);
            }
        }
    }

    // Add tool usage hint to system prompt when tools are available
    let has_injected_tools = body.get("tools").and_then(|t| t.as_array()).map_or(false, |a| !a.is_empty());
    if has_injected_tools {
        let tool_names: Vec<String> = body["tools"].as_array()
            .map(|arr| arr.iter()
                .filter_map(|t| t["function"]["name"].as_str().map(|s| s.to_string()))
                .collect())
            .unwrap_or_default();

        if let Some(messages) = body["messages"].as_array_mut() {
            let mut hint = format!(
                "\n\nYou have access to these tools: {}. When the user asks you to perform an action that matches a tool, you MUST call that tool. Do not describe what you would do — actually do it.",
                tool_names.join(", ")
            );

            // Detect send_message intent — force tool usage
            let last_user_msg = messages.iter().rev()
                .find(|m| m["role"] == "user")
                .and_then(|m| m["content"].as_str())
                .unwrap_or("")
                .to_lowercase();
            let msg_patterns = ["send_message", "send a message", "ask ", "tell ", "message ", "talk to "];
            if tool_names.contains(&"send_message".to_string()) && msg_patterns.iter().any(|p| last_user_msg.contains(p)) {
                hint.push_str("\n\nIMPORTANT: The user is requesting communication with another agent. You MUST use the send_message tool. Do NOT answer the question yourself — delegate it to the other agent.");
            }

            if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
                if let Some(content) = system_msg["content"].as_str() {
                    system_msg["content"] = serde_json::Value::String(format!("{}{}", content, hint));
                }
            }
        }
    }

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

    loop {
        iteration += 1;
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

        // CHECK FOR TOOL CALLS
        let assistant_msg = &response_body["choices"][0]["message"];
        let tool_calls = assistant_msg.get("tool_calls").and_then(|tc| tc.as_array());

        if let Some(tool_calls) = tool_calls {
            if tool_calls.is_empty() {
                let msgs = body["messages"].as_array().map(|a| a.to_vec());
                return finish_task(state.clone(), &agent.id, &task_id, true, total_cost, response_body, Some(&provider), msgs.as_deref()).await;
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
            let msgs = body["messages"].as_array().map(|a| a.to_vec());
            return finish_task(state.clone(), &agent.id, &task_id, true, total_cost, response_body, Some(&provider), msgs.as_deref()).await;
        }
    }
}

/// Stream an LLM response as SSE to the client. Used for simple chats (no tools).
async fn stream_llm_response(
    state: Arc<AppState>,
    client: &reqwest::Client,
    llm_url: &str,
    provider: &crate::providers::Provider,
    body: &serde_json::Value,
    agent_id: &str,
    task_id: &str,
) -> Response {
    let llm_response = match client
        .post(llm_url)
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .header("Content-Type", "application/json")
        .json(body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let db = state.db.lock().await;
            let _ = registry::update_agent_status(&db, agent_id, "idle");
            emit_status(&state, agent_id, "idle");
            emit_refresh(&state);
            return error_response(StatusCode::BAD_GATEWAY, &format!("could not reach LLM API: {}", e));
        }
    };

    let llm_status = llm_response.status();
    if !llm_status.is_success() {
        let db = state.db.lock().await;
        let _ = registry::update_agent_status(&db, agent_id, "idle");
        emit_status(&state, agent_id, "idle");
        let error_text = llm_response.text().await.unwrap_or_default();
        if llm_status == reqwest::StatusCode::UNAUTHORIZED {
            if let Some(handle) = &state.app_handle {
                let _ = handle.emit("toast", serde_json::json!({"type":"error","message":"Invalid API key."}));
            }
            return error_response(StatusCode::UNAUTHORIZED, "Invalid API key.");
        }
        return error_response(StatusCode::BAD_GATEWAY, &format!("LLM API returned {}. {}", llm_status, error_text));
    }

    // Check if LLM actually returned SSE (some providers fall back to JSON)
    let content_type = llm_response.headers().get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !content_type.contains("text/event-stream") {
        // LLM returned JSON, not SSE — handle as non-streaming
        match llm_response.json::<serde_json::Value>().await {
            Ok(response_body) => {
                let content = response_body["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string();
                {
                    let summary = format!("LLM: {}", content.chars().take(200).collect::<String>());
                    let db = state.db.lock().await;
                    let _ = episodic::insert_episode(&db, &Episode {
                        id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id.into(),
                        created_at: chrono::Utc::now().to_rfc3339(), event_type: "llm_response".into(),
                        summary: summary.clone(),
                        raw_data: Some(serde_json::to_string(&response_body).unwrap_or_default()),
                        task_id: Some(task_id.into()), outcome: Some("success".into()),
                        tokens_used: 0, cost_cents: 0,
                    });
                    let _ = audit::log_action(&db, &AuditEntry {
                        id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id.into(),
                        created_at: chrono::Utc::now().to_rfc3339(), action_type: "llm_response".into(),
                        action_detail: redact_secrets(&summary),
                        permission_result: "allowed".into(),
                        result: None, duration_ms: None, cost_cents: 0, error: None,
                    });
                    let _ = registry::update_agent_status(&db, agent_id, "idle");
                    let _ = registry::increment_task_counts(&db, agent_id, true, 0);
                    let _ = episodic::insert_episode(&db, &Episode {
                        id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id.into(),
                        created_at: chrono::Utc::now().to_rfc3339(), event_type: "task_end".into(),
                        summary: "Task completed successfully".into(), raw_data: None,
                        task_id: Some(task_id.into()), outcome: Some("success".into()),
                        tokens_used: 0, cost_cents: 0,
                    });
                    let _ = audit::log_action(&db, &AuditEntry {
                        id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id.into(),
                        created_at: chrono::Utc::now().to_rfc3339(), action_type: "task_end".into(),
                        action_detail: "Task completed successfully".into(),
                        permission_result: "allowed".into(),
                        result: None, duration_ms: None, cost_cents: 0, error: None,
                    });
                }
                emit_status(&state, agent_id, "idle");
                emit_refresh(&state);
                return Json(response_body).into_response();
            }
            Err(e) => {
                return error_response(StatusCode::BAD_GATEWAY, &format!("Failed to parse LLM response: {}", e));
            }
        }
    }

    // SSE streaming path — forward chunks from LLM to client
    let agent_id_owned = agent_id.to_string();
    let task_id_owned = task_id.to_string();
    let provider_clone = provider.clone();
    let original_messages: Vec<serde_json::Value> = body["messages"].as_array().cloned().unwrap_or_default();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<SseEvent, std::convert::Infallible>>(64);

    // Move Arc<AppState> into the spawned task
    let state_clone = state.clone();

    tokio::spawn(async move {
        let mut byte_stream = llm_response.bytes_stream();
        let mut accumulated_content = String::new();
        let mut buffer = String::new();

        while let Some(chunk_result) = byte_stream.next().await {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(_) => break,
            };
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // Process complete lines
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                if line == "data: [DONE]" {
                    let _ = tx.send(Ok(SseEvent::default().data("[DONE]"))).await;
                    // Log the accumulated response
                    {
                        let db = state_clone.db.lock().await;
                        let summary = format!("LLM: {}", accumulated_content.chars().take(200).collect::<String>());
                        let _ = episodic::insert_episode(&db, &Episode {
                            id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id_owned.clone(),
                            created_at: chrono::Utc::now().to_rfc3339(), event_type: "llm_response".into(),
                            summary: summary.clone(), raw_data: Some(accumulated_content.clone()),
                            task_id: Some(task_id_owned.clone()), outcome: Some("success".into()),
                            tokens_used: 0, cost_cents: 0,
                        });
                        let _ = audit::log_action(&db, &AuditEntry {
                            id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id_owned.clone(),
                            created_at: chrono::Utc::now().to_rfc3339(), action_type: "llm_response".into(),
                            action_detail: redact_secrets(&summary),
                            permission_result: "allowed".into(),
                            result: None, duration_ms: None, cost_cents: 0, error: None,
                        });
                        let _ = registry::update_agent_status(&db, &agent_id_owned, "idle");
                        let _ = registry::increment_task_counts(&db, &agent_id_owned, true, 0);
                        let _ = episodic::insert_episode(&db, &Episode {
                            id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id_owned.clone(),
                            created_at: chrono::Utc::now().to_rfc3339(), event_type: "task_end".into(),
                            summary: "Task completed successfully".into(), raw_data: None,
                            task_id: Some(task_id_owned.clone()), outcome: Some("success".into()),
                            tokens_used: 0, cost_cents: 0,
                        });
                        let _ = audit::log_action(&db, &AuditEntry {
                            id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id_owned.clone(),
                            created_at: chrono::Utc::now().to_rfc3339(), action_type: "task_end".into(),
                            action_detail: "Task completed successfully".into(),
                            permission_result: "allowed".into(),
                            result: None, duration_ms: None, cost_cents: 0, error: None,
                        });
                    }
                    if let Some(handle) = &state_clone.app_handle {
                        let _ = handle.emit("agent-status-change", serde_json::json!({"id": agent_id_owned, "status": "idle"}));
                        let _ = handle.emit("activity-refresh", ());
                    }

                    // Record growth metrics
                    {
                        let db = state_clone.db.lock().await;
                        let _ = crate::metrics::record_metric(&db, &agent_id_owned);
                    }

                    // Trigger background features (reflection, profile, goals, journal)
                    let config = state_clone.config.read().await;
                    let alive_mode = config.ui.alive_mode;
                    let reflection_enabled = config.llm.self_reflection_enabled;
                    drop(config);

                    if alive_mode {
                        let (total_tasks, active_goal_count) = {
                            let db = state_clone.db.lock().await;
                            let total = registry::get_agent(&db, &agent_id_owned).ok().flatten().map(|a| a.total_tasks).unwrap_or(0);
                            let goals = crate::goals::count_active_goals(&db, &agent_id_owned).unwrap_or(0);
                            (total, goals)
                        };

                        let should_reflect = reflection_enabled && original_messages.len() >= 2
                            && true;

                        if should_reflect {
                            crate::reflection::spawn_reflection(
                                state_clone.clone(), agent_id_owned.clone(), provider_clone.clone(),
                                original_messages.clone(), task_id_owned.clone(),
                            );
                        }

                        crate::profile::maybe_regenerate(
                            state_clone.clone(), agent_id_owned.clone(), provider_clone.clone(), total_tasks,
                        );

                        crate::goals::maybe_generate_goals(
                            state_clone.clone(), agent_id_owned.clone(), provider_clone.clone(),
                            total_tasks, active_goal_count,
                        );

                        let db = state_clone.db.lock().await;
                        if crate::journal::should_write_journal(&db, &agent_id_owned) {
                            drop(db);
                            crate::journal::spawn_journal_synthesis(
                                state_clone.clone(), agent_id_owned.clone(), provider_clone.clone(),
                            );
                        }

                        // Self-verification
                        if original_messages.len() >= 2 {
                            crate::self_verify::spawn_verification(
                                state_clone.clone(), agent_id_owned.clone(), provider_clone.clone(),
                                original_messages.clone(), task_id_owned.clone(), None,
                            );
                        }
                    }

                    return; // Stream done
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    // Extract content delta for accumulation
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                            accumulated_content.push_str(content);
                        }
                    }
                    // Forward the raw SSE data to the client
                    let _ = tx.send(Ok(SseEvent::default().data(data.to_string()))).await;
                }
            }
        }

        // Stream ended without [DONE] — still log what we have
        {
            let db = state_clone.db.lock().await;
            let _ = registry::update_agent_status(&db, &agent_id_owned, "idle");
            let _ = registry::increment_task_counts(&db, &agent_id_owned, true, 0);
            if !accumulated_content.is_empty() {
                let _ = episodic::insert_episode(&db, &Episode {
                    id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id_owned.clone(),
                    created_at: chrono::Utc::now().to_rfc3339(), event_type: "llm_response".into(),
                    summary: format!("LLM (partial): {}", accumulated_content.chars().take(200).collect::<String>()),
                    raw_data: Some(accumulated_content), task_id: Some(task_id_owned.clone()),
                    outcome: Some("partial".into()), tokens_used: 0, cost_cents: 0,
                });
            }
            let _ = episodic::insert_episode(&db, &Episode {
                id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id_owned.clone(),
                created_at: chrono::Utc::now().to_rfc3339(), event_type: "task_end".into(),
                summary: "Task completed (stream ended)".into(), raw_data: None,
                task_id: Some(task_id_owned), outcome: Some("success".into()),
                tokens_used: 0, cost_cents: 0,
            });
        }
        if let Some(handle) = &state_clone.app_handle {
            let _ = handle.emit("agent-status-change", serde_json::json!({"id": agent_id_owned, "status": "idle"}));
            let _ = handle.emit("activity-refresh", ());
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

async fn finish_task(
    state: Arc<AppState>,
    agent_id: &str,
    task_id: &str,
    success: bool,
    cost_cents: i64,
    response: serde_json::Value,
    // For reflection: the provider and messages from the conversation
    provider: Option<&crate::providers::Provider>,
    messages: Option<&[serde_json::Value]>,
) -> Response {
    {
        let db = state.db.lock().await;
        let _ = registry::update_agent_status(&db, agent_id, "idle");
        let _ = registry::increment_task_counts(&db, agent_id, success, cost_cents);
        let summary: String = if success { "Task completed successfully" } else { "Task completed with errors" }.into();
        let _ = episodic::insert_episode(&db, &Episode {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            event_type: "task_end".into(),
            summary: summary.clone(),
            raw_data: None,
            task_id: Some(task_id.into()),
            outcome: Some(if success { "success" } else { "failure" }.into()),
            tokens_used: 0,
            cost_cents,
        });
        let _ = audit::log_action(&db, &AuditEntry {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            action_type: "task_end".into(),
            action_detail: summary,
            permission_result: "allowed".into(),
            result: None, duration_ms: None, cost_cents, error: None,
        });
    }

    emit_status(&state, agent_id, "idle");
    emit_refresh(&state);

    // Record growth metrics (UPSERT today's snapshot)
    {
        let db = state.db.lock().await;
        let _ = crate::metrics::record_metric(&db, agent_id);
    }

    // Background features only run in Alive Mode
    let config = state.config.read().await;
    let alive_mode = config.ui.alive_mode;
    let reflection_enabled = config.llm.self_reflection_enabled;
    drop(config);

    if success && alive_mode {
        if let (Some(provider), Some(msgs)) = (provider, messages) {
            let (total_tasks, active_goal_count) = {
                let db = state.db.lock().await;
                let total = registry::get_agent(&db, agent_id).ok().flatten().map(|a| a.total_tasks).unwrap_or(0);
                let goals = crate::goals::count_active_goals(&db, agent_id).unwrap_or(0);
                (total, goals)
            };

            // Reflection: every task in Alive Mode
            let should_reflect = reflection_enabled && msgs.len() >= 2;
            if should_reflect {
                crate::reflection::spawn_reflection(
                    state.clone(), agent_id.to_string(), provider.clone(),
                    msgs.to_vec(), task_id.to_string(),
                );
            }

            crate::profile::maybe_regenerate(
                state.clone(), agent_id.to_string(), provider.clone(), total_tasks,
            );

            crate::goals::maybe_generate_goals(
                state.clone(), agent_id.to_string(), provider.clone(),
                total_tasks, active_goal_count,
            );

            // Journal (every 3rd task)
            let db = state.db.lock().await;
            if crate::journal::should_write_journal(&db, agent_id) {
                drop(db);
                crate::journal::spawn_journal_synthesis(
                    state.clone(), agent_id.to_string(), provider.clone(),
                );
            }
        }
    }

    // Reflection on FAILED tasks runs even in Core Mode (if reflection enabled)
    if !success && reflection_enabled {
        if let (Some(provider), Some(msgs)) = (provider, messages) {
            if msgs.len() >= 2 {
                crate::reflection::spawn_reflection(
                    state.clone(), agent_id.to_string(), provider.clone(),
                    msgs.to_vec(), task_id.to_string(),
                );
            }
        }
    }

    // Self-verification: agent rates its own output quality (Alive Mode only)
    if alive_mode {
        if let (Some(provider), Some(msgs)) = (provider, messages) {
            if msgs.len() >= 2 {
                crate::self_verify::spawn_verification(
                    state.clone(), agent_id.to_string(), provider.clone(),
                    msgs.to_vec(), task_id.to_string(), None,
                );
            }
        }
    }

    // Add task_id to response (both JSON body and header) for rating support
    let mut response = response;
    response["greencube_task_id"] = serde_json::Value::String(task_id.to_string());
    let mut resp = Json(response).into_response();
    resp.headers_mut().insert(
        "x-greencube-task-id",
        axum::http::HeaderValue::from_str(task_id).unwrap_or_else(|_| axum::http::HeaderValue::from_static("")),
    );
    resp
}

async fn execute_tool_call(state: &AppState, agent_id: &str, tool_name: &str, arguments: &serde_json::Value) -> String {
    // Tool memory: check for recent identical calls
    let previous = {
        let db = state.db.lock().await;
        crate::tool_memory::lookup_recent(&db, agent_id, tool_name, arguments).ok().flatten()
    };

    let mut prefix = String::new();
    if let Some(ref prev) = previous {
        if prev.success {
            prefix = format!("[Note: identical call was made at {} and returned: {}]\n\n", prev.created_at, prev.result.chars().take(200).collect::<String>());
        } else {
            prefix = format!("[Warning: this same call failed at {} with: {}]\n\n", prev.created_at, prev.result.chars().take(200).collect::<String>());
        }
    }

    let result = match tool_name {
        "shell" => {
            let command = match arguments["command"].as_str() {
                Some(c) => c,
                None => return "Error: shell tool requires 'command' argument".into(),
            };
            execute_shell(state, command).await
        }
        "read_file" => {
            let path = match arguments["path"].as_str() {
                Some(p) => p,
                None => return "Error: read_file requires 'path' argument".into(),
            };
            execute_shell(state, &format!("cat {}", sandbox_docker::shell_escape(path))).await
        }
        "write_file" => {
            let path = match arguments["path"].as_str() {
                Some(p) => p,
                None => return "Error: write_file requires 'path' argument".into(),
            };
            let content = match arguments["content"].as_str() {
                Some(c) => c,
                None => return "Error: write_file requires 'content' argument".into(),
            };
            // SECURITY: Use base64 encoding to prevent here-doc injection.
            // If content contained 'GREENCUBE_EOF', it would break out of the heredoc
            // and execute arbitrary commands. Base64 eliminates this entirely.
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
            execute_shell(state, &format!("echo {} | base64 -d > {}", encoded, sandbox_docker::shell_escape(path))).await
        }
        "http_get" => {
            let url = match arguments["url"].as_str() {
                Some(u) => u,
                None => return "Error: http_get requires 'url' argument".into(),
            };
            execute_shell(state, &format!("curl -sS {}", sandbox_docker::shell_escape(url))).await
        }
        "update_context" => {
            let content = match arguments["content"].as_str() {
                Some(c) => c,
                None => return "Error: update_context requires 'content' argument".into(),
            };
            let db = state.db.lock().await;
            match crate::context::set_context(&db, agent_id, content) {
                Ok(()) => "Context updated successfully.".into(),
                Err(e) => format!("Error updating context: {}", e),
            }
        }
        "set_reminder" => {
            let prompt = match arguments["prompt"].as_str() {
                Some(p) => p,
                None => return "Error: set_reminder requires 'prompt' argument".into(),
            };
            let minutes = arguments["minutes_from_now"].as_i64().unwrap_or(60);
            let provider_id = {
                let db = state.db.lock().await;
                let agent = crate::identity::registry::get_agent(&db, agent_id).ok().flatten();
                agent.and_then(|a| a.provider_id)
            };
            let db = state.db.lock().await;
            match crate::task_queue::queue_reminder(&db, agent_id, prompt, minutes, provider_id.as_deref()) {
                Ok(_) => format!("Reminder set. I will execute '{}' in {} minutes.", prompt, minutes),
                Err(e) => format!("Error setting reminder: {}", e),
            }
        }
        "spawn_specialist" => {
            let domain = match arguments["domain"].as_str() {
                Some(d) => d,
                None => return "Error: spawn_specialist requires 'domain' argument".into(),
            };
            let alive = state.config.read().await.ui.alive_mode;
            if !alive { return "spawn_specialist requires Alive Mode. Enable in Settings.".into(); }
            match crate::spawn::execute_spawn(state, agent_id, domain).await {
                Ok(child_name) => format!(
                    "Successfully created specialist: {}. You can delegate {} tasks to them using send_message(to=\"{}\", content=\"...\").",
                    child_name, domain, child_name
                ),
                Err(e) => format!("Could not spawn specialist: {}", e),
            }
        }
        "send_message" => {
            let to_name = match arguments["to"].as_str() {
                Some(t) => t,
                None => return "Error: send_message requires 'to' argument (agent name)".into(),
            };
            let content = match arguments["content"].as_str() {
                Some(c) => c,
                None => return "Error: send_message requires 'content' argument".into(),
            };
            let depth = arguments["_depth"].as_u64().unwrap_or(0) as u32;
            match crate::agent_messages::send_message(state, agent_id, to_name, content, depth).await {
                Ok(response) => format!("Response from {}: {}", to_name, response),
                Err(e) => format!("Error sending message to {}: {}", to_name, e),
            }
        }
        _ => format!("Error: unknown tool '{}'", tool_name),
    };

    // Store tool result for future memory
    if tool_name != "update_context" { // Don't cache context updates
        let success = !result.starts_with("Error:");
        let db = state.db.lock().await;
        let _ = crate::tool_memory::store_result(&db, agent_id, tool_name, arguments, &result, success);
    }

    format!("{}{}", prefix, result)
}

async fn execute_shell(state: &AppState, command: &str) -> String {
    let docker = state.docker.read().await;
    let docker = match docker.as_ref() {
        Some(d) => d,
        None => return "Error: Docker is not available. Install Docker to enable tool execution.".into(),
    };
    let config = state.config.read().await;
    let opts = SandboxOptions {
        image: config.sandbox.image.clone(),
        cpu_limit_cores: config.sandbox.cpu_limit_cores,
        memory_limit_mb: config.sandbox.memory_limit_mb,
        timeout_seconds: config.sandbox.timeout_seconds,
        network_enabled: config.sandbox.network_enabled,
    };
    match sandbox_docker::execute_in_sandbox(docker, command, &opts).await {
        Ok(result) => {
            if result.timed_out {
                format!("Command timed out after {} seconds", opts.timeout_seconds)
            } else {
                format!("Exit code: {}\nStdout:\n{}\nStderr:\n{}", result.exit_code, result.stdout, result.stderr)
            }
        }
        Err(e) => format!("Sandbox error: {}", e),
    }
}

fn inject_memories(messages: &mut Vec<serde_json::Value>, memories: &[Episode]) {
    if memories.is_empty() { return; }
    let memory_text = memories.iter()
        .map(|ep| format!("[Memory from {}] {}: {}", ep.created_at, ep.event_type, ep.summary))
        .collect::<Vec<_>>()
        .join("\n");
    let injection = format!("\n\n--- Relevant memories from past tasks ---\n{}\n--- End memories ---", memory_text);
    if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
        if let Some(content) = system_msg["content"].as_str() {
            system_msg["content"] = serde_json::Value::String(format!("{}{}", content, injection));
        }
    } else {
        messages.insert(0, serde_json::json!({"role": "system", "content": injection}));
    }
}

/// Build OpenAI-format tool definitions for the agent's allowed tools.
fn build_tool_definitions(tools_allowed: &[String]) -> Vec<serde_json::Value> {
    let all_tools: Vec<(&str, &str, serde_json::Value)> = vec![
        ("shell", "Execute a shell command in a sandboxed Docker container. Use this for running code, scripts, or system commands.", serde_json::json!({
            "type": "object",
            "properties": {"command": {"type": "string", "description": "The shell command to execute"}},
            "required": ["command"]
        })),
        ("read_file", "Read the contents of a file in the sandbox.", serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string", "description": "Path to the file to read"}},
            "required": ["path"]
        })),
        ("write_file", "Write content to a file in the sandbox.", serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to write to"},
                "content": {"type": "string", "description": "Content to write"}
            },
            "required": ["path", "content"]
        })),
        ("http_get", "Make an HTTP GET request to a URL.", serde_json::json!({
            "type": "object",
            "properties": {"url": {"type": "string", "description": "The URL to fetch"}},
            "required": ["url"]
        })),
        ("update_context", "Update your working notes/scratchpad. Use this to save important context for future tasks.", serde_json::json!({
            "type": "object",
            "properties": {"content": {"type": "string", "description": "The new content for your working notes"}},
            "required": ["content"]
        })),
        ("send_message", "Send a message to another agent and get their response.", serde_json::json!({
            "type": "object",
            "properties": {
                "to": {"type": "string", "description": "Name of the agent to message"},
                "content": {"type": "string", "description": "The message to send"}
            },
            "required": ["to", "content"]
        })),
        ("set_reminder", "Set a reminder to do something later. The task will execute automatically at the specified time.", serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {"type": "string", "description": "What to do when the reminder fires"},
                "minutes_from_now": {"type": "integer", "description": "Minutes from now to execute (max 1440)"}
            },
            "required": ["prompt", "minutes_from_now"]
        })),
        ("spawn_specialist", "Create a specialist agent for a domain you struggle with. The specialist inherits your knowledge in that domain.", serde_json::json!({
            "type": "object",
            "properties": {"domain": {"type": "string", "description": "The domain to specialize in (e.g., 'css', 'python', 'database')"}},
            "required": ["domain"]
        })),
    ];

    all_tools.into_iter()
        .filter(|(name, _, _)| tools_allowed.iter().any(|t| t == name))
        .map(|(name, description, parameters)| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": description,
                    "parameters": parameters
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod security_tests {
    use super::redact_secrets;

    #[test]
    fn test_redact_bearer_token() {
        let input = r#"curl -H "Authorization: Bearer sk-abc123def456""#;
        let result = redact_secrets(input);
        assert!(!result.contains("sk-abc123def456"), "Bearer token not redacted: {}", result);
        assert!(result.contains("[REDACTED"), "Missing redaction marker: {}", result);
    }

    #[test]
    fn test_redact_api_key_pattern() {
        let input = "api_key=sk-proj-abcdefghijklmnopqrstuvwxyz";
        let result = redact_secrets(input);
        assert!(!result.contains("abcdefghijklmnopqrstuvwxyz"), "API key not redacted: {}", result);
    }

    #[test]
    fn test_redact_sk_prefix() {
        let input = "Using key sk-1234567890abcdefghij1234567890 for requests";
        let result = redact_secrets(input);
        assert!(result.contains("[REDACTED_KEY]"), "sk- pattern not redacted: {}", result);
    }

    #[test]
    fn test_no_false_positive_on_normal_text() {
        let input = "This is a normal shell command: ls -la /tmp";
        let result = redact_secrets(input);
        assert_eq!(result, input, "Normal text was modified");
    }
}

#[cfg(test)]
#[path = "integration_test.rs"]
mod integration_test;
