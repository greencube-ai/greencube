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

    // 4. INJECT MEMORIES
    let memory_enabled = state.config.read().await.llm.memory_injection_enabled;
    if memory_enabled {
        if let Some(messages) = body["messages"].as_array_mut() {
            if let Some(last_user_msg) = messages.iter().rev().find(|m| m["role"] == "user").and_then(|m| m["content"].as_str()) {
                let db = state.db.lock().await;
                if let Ok(memories) = episodic::recall_relevant_episodes(&db, &agent.id, last_user_msg, 5) {
                    inject_memories(messages, &memories);
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

    let config = state.config.read().await.clone();
    let client = reqwest::Client::new();
    let llm_url = format!("{}/chat/completions", config.llm.api_base_url);

    // STREAMING PATH: If client wants streaming AND no tools, stream directly from the first call
    if wants_stream && !has_tools {
        body["stream"] = serde_json::Value::Bool(true);
        return stream_llm_response(state.clone(), &client, &llm_url, &config, &body, &agent.id, &task_id).await;
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
            .header("Authorization", format!("Bearer {}", config.llm.api_key))
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
                return finish_task(&state, &agent.id, &task_id, true, total_cost, response_body).await;
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
                    execute_tool_call(&state, func_name, &func_args).await
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
            // No tool calls — return final response as-is (even if client wanted streaming,
            // we already have the complete response from the tool-call loop, no point re-sending)
            return finish_task(&state, &agent.id, &task_id, true, total_cost, response_body).await;
        }
    }
}

/// Stream an LLM response as SSE to the client. Used for simple chats (no tools).
async fn stream_llm_response(
    state: Arc<AppState>,
    client: &reqwest::Client,
    llm_url: &str,
    config: &crate::config::AppConfig,
    body: &serde_json::Value,
    agent_id: &str,
    task_id: &str,
) -> Response {
    let llm_response = match client
        .post(llm_url)
        .header("Authorization", format!("Bearer {}", config.llm.api_key))
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
    state: &AppState,
    agent_id: &str,
    task_id: &str,
    success: bool,
    cost_cents: i64,
    response: serde_json::Value,
) -> Response {
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
    drop(db); // Release lock before emitting

    emit_status(state, agent_id, "idle");
    emit_refresh(state);

    Json(response).into_response()
}

async fn execute_tool_call(state: &AppState, tool_name: &str, arguments: &serde_json::Value) -> String {
    match tool_name {
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
        _ => format!("Error: unknown tool '{}'", tool_name),
    }
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
