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
    }
    emit_status(&state, &agent.id, "active");
    emit_refresh(&state);

    // 3a. INJECT COMMANDMENTS (always first, non-negotiable)
    if let Some(messages) = body["messages"].as_array_mut() {
        crate::commandments::inject_commandments(messages);
    }

    // 3b. INJECT WORKING CONTEXT (scratchpad) + DYNAMIC PROFILE
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

    // 5. CHECK PERMISSIONS
    if let Some(tools) = body["tools"].as_array() {
        let filtered: Vec<serde_json::Value> = tools.iter()
            .filter(|t| t["function"]["name"].as_str().map(|name| permissions::check_tool_permission(&agent, name)).unwrap_or(false))
            .cloned()
            .collect();
        body["tools"] = serde_json::Value::Array(filtered);
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

        // Log LLM response episode
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
                summary: response_summary,
                raw_data: Some(serde_json::to_string(&response_body).unwrap_or_default()),
                task_id: Some(task_id.clone()), outcome: Some("success".into()),
                tokens_used: total_tokens, cost_cents: total_cost,
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
                        action_detail: serde_json::json!({"tool": func_name, "arguments": func_args}).to_string(),
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
                    let db = state.db.lock().await;
                    let _ = episodic::insert_episode(&db, &Episode {
                        id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id.into(),
                        created_at: chrono::Utc::now().to_rfc3339(), event_type: "llm_response".into(),
                        summary: format!("LLM: {}", content.chars().take(200).collect::<String>()),
                        raw_data: Some(serde_json::to_string(&response_body).unwrap_or_default()),
                        task_id: Some(task_id.into()), outcome: Some("success".into()),
                        tokens_used: 0, cost_cents: 0,
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
                            summary, raw_data: Some(accumulated_content.clone()),
                            task_id: Some(task_id_owned.clone()), outcome: Some("success".into()),
                            tokens_used: 0, cost_cents: 0,
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
                    }
                    if let Some(handle) = &state_clone.app_handle {
                        let _ = handle.emit("agent-status-change", serde_json::json!({"id": agent_id_owned, "status": "idle"}));
                        let _ = handle.emit("activity-refresh", ());
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
        let _ = episodic::insert_episode(&db, &Episode {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            event_type: "task_end".into(),
            summary: if success { "Task completed successfully" } else { "Task completed with errors" }.into(),
            raw_data: None,
            task_id: Some(task_id.into()),
            outcome: Some(if success { "success" } else { "failure" }.into()),
            tokens_used: 0,
            cost_cents,
        });
    }

    emit_status(&state, agent_id, "idle");
    emit_refresh(&state);

    // Record growth metrics (UPSERT today's snapshot)
    {
        let db = state.db.lock().await;
        let _ = crate::metrics::record_metric(&db, agent_id);
    }

    // Spawn self-reflection if enabled and task succeeded
    if success {
        if let (Some(provider), Some(msgs)) = (provider, messages) {
            let reflection_enabled = state.config.read().await.llm.self_reflection_enabled;
            if reflection_enabled && msgs.len() >= 2 {
                crate::reflection::spawn_reflection(
                    state.clone(),
                    agent_id.to_string(),
                    provider.clone(),
                    msgs.to_vec(),
                    task_id.to_string(),
                );
            }

            // Check if dynamic profile needs regeneration (every 5 tasks)
            let (total_tasks, active_goal_count) = {
                let db = state.db.lock().await;
                let total = registry::get_agent(&db, agent_id).ok().flatten().map(|a| a.total_tasks).unwrap_or(0);
                let goals = crate::goals::count_active_goals(&db, agent_id).unwrap_or(0);
                (total, goals)
            };
            crate::profile::maybe_regenerate(
                state.clone(),
                agent_id.to_string(),
                provider.clone(),
                total_tasks,
            );

            // Check if goals should be generated
            crate::goals::maybe_generate_goals(
                state.clone(),
                agent_id.to_string(),
                provider.clone(),
                total_tasks,
                active_goal_count,
            );
        }
    }

    Json(response).into_response()
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
            execute_shell(state, &format!("cat > {} << 'GREENCUBE_EOF'\n{}\nGREENCUBE_EOF", sandbox_docker::shell_escape(path), content)).await
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

#[cfg(test)]
#[path = "integration_test.rs"]
mod integration_test;
