use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use std::sync::Arc;
use tauri::Emitter; // Required for .emit() on AppHandle in Tauri 2.0

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

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 1. RECEIVE REQUEST — extract agent_id
    let agent_id = headers
        .get("x-agent-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Override streaming — v0.1 only supports non-streaming
    body["stream"] = serde_json::Value::Bool(false);

    // 2. RESOLVE AGENT
    let agent = {
        let db = state.db.lock().await;
        if let Some(ref id) = agent_id {
            match registry::get_agent(&db, id) {
                Ok(Some(a)) => a,
                Ok(None) => {
                    return Err((
                        StatusCode::NOT_FOUND,
                        Json(serde_json::json!({ "error": format!("agent not found: {}", id) })),
                    ))
                }
                Err(e) => {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": e.to_string() })),
                    ))
                }
            }
        } else {
            // No agent ID — use or create "default" agent
            match registry::get_agent_by_name(&db, "default") {
                Ok(Some(a)) => a,
                Ok(None) => {
                    // Auto-create default agent
                    let tools = vec![
                        "shell".into(),
                        "read_file".into(),
                        "write_file".into(),
                        "http_get".into(),
                    ];
                    match registry::create_agent(&db, "default", "You are a helpful assistant.", &tools) {
                        Ok(a) => a,
                        Err(e) => {
                            return Err((
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(serde_json::json!({ "error": format!("failed to create default agent: {}", e) })),
                            ))
                        }
                    }
                }
                Err(e) => {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": e.to_string() })),
                    ))
                }
            }
        }
    };

    // 3. LOG TASK START — include user's message in summary for memory recall
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
        let _ = episodic::insert_episode(
            &db,
            &Episode {
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
            },
        );
    }

    // Emit event to frontend
    if let Some(handle) = &state.app_handle {
        let _ = handle.emit("agent-status-change", serde_json::json!({
            "id": agent.id,
            "status": "active"
        }));
    }

    // 4. INJECT MEMORIES (only if enabled in config)
    let memory_enabled = state.config.read().await.llm.memory_injection_enabled;
    if memory_enabled {
        if let Some(messages) = body["messages"].as_array_mut() {
            if let Some(last_user_msg) = messages
                .iter()
                .rev()
                .find(|m| m["role"] == "user")
                .and_then(|m| m["content"].as_str())
            {
                let db = state.db.lock().await;
                if let Ok(memories) = episodic::recall_relevant_episodes(&db, &agent.id, last_user_msg, 5) {
                    inject_memories(messages, &memories);
                }
            }
        }
    }

    // 5. CHECK PERMISSIONS — filter out tools not in agent's allowed list
    if let Some(tools) = body["tools"].as_array() {
        let filtered: Vec<serde_json::Value> = tools
            .iter()
            .filter(|t| {
                t["function"]["name"]
                    .as_str()
                    .map(|name| permissions::check_tool_permission(&agent, name))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        body["tools"] = serde_json::Value::Array(filtered);
    }

    // 6-8. FORWARD TO LLM + TOOL CALL LOOP
    let config = state.config.read().await.clone();
    let client = reqwest::Client::new();
    let mut total_tokens = 0i64;
    let mut total_cost = 0i64;
    let mut iteration = 0;

    loop {
        iteration += 1;
        if iteration > MAX_TOOL_ITERATIONS {
            // Set agent back to idle
            let db = state.db.lock().await;
            let _ = registry::update_agent_status(&db, &agent.id, "idle");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "tool call loop exceeded 10 iterations. Possible infinite loop." })),
            ));
        }

        // Forward to LLM
        let llm_url = format!("{}/chat/completions", config.llm.api_base_url);
        let llm_response = client
            .post(&llm_url)
            .header("Authorization", format!("Bearer {}", config.llm.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await;

        let llm_response = match llm_response {
            Ok(r) => r,
            Err(e) => {
                let db = state.db.lock().await;
                let _ = registry::update_agent_status(&db, &agent.id, "idle");
                let _ = episodic::insert_episode(&db, &Episode {
                    id: uuid::Uuid::new_v4().to_string(),
                    agent_id: agent.id.clone(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    event_type: "error".into(),
                    summary: format!("LLM API unreachable: {}", e),
                    raw_data: None,
                    task_id: Some(task_id.clone()),
                    outcome: Some("failure".into()),
                    tokens_used: 0,
                    cost_cents: 0,
                });
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("could not reach LLM API at {}. Check your API key and network.", llm_url) })),
                ));
            }
        };

        let llm_status = llm_response.status();
        if !llm_status.is_success() {
            let db = state.db.lock().await;
            let _ = registry::update_agent_status(&db, &agent.id, "idle");
            let error_text = llm_response.text().await.unwrap_or_default();

            // Specific handling for 401 — emit toast so UI shows API key error
            if llm_status == reqwest::StatusCode::UNAUTHORIZED {
                if let Some(handle) = &state.app_handle {
                    let _ = handle.emit("toast", serde_json::json!({
                        "type": "error",
                        "message": "Invalid API key. Check your LLM API key in Settings."
                    }));
                }
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({ "error": "Invalid API key. Update your key in Settings." })),
                ));
            }

            return Err((
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("LLM API returned {}. {}", llm_status, error_text) })),
            ));
        }

        let response_body: serde_json::Value = match llm_response.json().await {
            Ok(v) => v,
            Err(e) => {
                let db = state.db.lock().await;
                let _ = registry::update_agent_status(&db, &agent.id, "idle");
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("Failed to parse LLM response: {}", e) })),
                ));
            }
        };

        // Track tokens
        if let Some(usage) = response_body.get("usage") {
            let tokens = usage["total_tokens"].as_i64().unwrap_or(0);
            total_tokens += tokens;
            // Rough cost estimate: $0.01/1K tokens
            total_cost += tokens / 100;
        }

        // Log LLM response episode — include content snippet in summary for memory recall
        let response_content = response_body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect::<String>();
        let response_summary = if response_content.is_empty() {
            format!("LLM responded with tool calls (iteration {})", iteration)
        } else {
            format!("LLM: {}", response_content)
        };
        {
            let db = state.db.lock().await;
            let _ = episodic::insert_episode(&db, &Episode {
                id: uuid::Uuid::new_v4().to_string(),
                agent_id: agent.id.clone(),
                created_at: chrono::Utc::now().to_rfc3339(),
                event_type: "llm_response".into(),
                summary: response_summary,
                raw_data: Some(serde_json::to_string(&response_body).unwrap_or_default()),
                task_id: Some(task_id.clone()),
                outcome: Some("success".into()),
                tokens_used: total_tokens,
                cost_cents: total_cost,
            });
        }

        // 8. CHECK FOR TOOL CALLS
        let assistant_msg = &response_body["choices"][0]["message"];
        let tool_calls = assistant_msg.get("tool_calls").and_then(|tc| tc.as_array());

        if let Some(tool_calls) = tool_calls {
            if tool_calls.is_empty() {
                // No tool calls — return final response
                return finish_task(&state, &agent.id, &task_id, true, total_cost, response_body).await;
            }

            // Execute each tool call
            let mut tool_results = Vec::new();
            for tc in tool_calls {
                let tc_id = tc["id"].as_str().unwrap_or("unknown");
                let func_name = tc["function"]["name"].as_str().unwrap_or("unknown");
                let func_args: serde_json::Value = tc["function"]["arguments"]
                    .as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(serde_json::Value::Object(Default::default()));

                // Log audit entry
                {
                    let db = state.db.lock().await;
                    let _ = audit::log_action(&db, &AuditEntry {
                        id: uuid::Uuid::new_v4().to_string(),
                        agent_id: agent.id.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        action_type: "tool_call".into(),
                        action_detail: serde_json::json!({
                            "tool": func_name,
                            "arguments": func_args
                        }).to_string(),
                        permission_result: if permissions::check_tool_permission(&agent, func_name) { "allowed" } else { "denied" }.into(),
                        result: None,
                        duration_ms: None,
                        cost_cents: 0,
                        error: None,
                    });
                }

                // Emit activity event
                if let Some(handle) = &state.app_handle {
                    let _ = handle.emit("activity-update", serde_json::json!({
                        "agent_id": agent.id,
                        "action_type": "tool_call",
                        "tool": func_name,
                    }));
                }

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

            // Append assistant message + tool results to messages and loop
            if let Some(messages) = body["messages"].as_array_mut() {
                messages.push(assistant_msg.clone());
                messages.extend(tool_results);
            }
        } else {
            // No tool_calls field — return final response
            return finish_task(&state, &agent.id, &task_id, true, total_cost, response_body).await;
        }
    }
}

async fn finish_task(
    state: &AppState,
    agent_id: &str,
    task_id: &str,
    success: bool,
    cost_cents: i64,
    response: serde_json::Value,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
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

    if let Some(handle) = &state.app_handle {
        let _ = handle.emit("agent-status-change", serde_json::json!({
            "id": agent_id,
            "status": "idle"
        }));
    }

    Ok(Json(response))
}

async fn execute_tool_call(
    state: &AppState,
    tool_name: &str,
    arguments: &serde_json::Value,
) -> String {
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
            let command = format!("cat {}", sandbox_docker::shell_escape(path));
            execute_shell(state, &command).await
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
            let command = format!(
                "cat > {} << 'GREENCUBE_EOF'\n{}\nGREENCUBE_EOF",
                sandbox_docker::shell_escape(path),
                content
            );
            execute_shell(state, &command).await
        }
        "http_get" => {
            let url = match arguments["url"].as_str() {
                Some(u) => u,
                None => return "Error: http_get requires 'url' argument".into(),
            };
            let command = format!("curl -sS {}", sandbox_docker::shell_escape(url));
            execute_shell(state, &command).await
        }
        _ => format!("Error: unknown tool '{}'", tool_name),
    }
}

async fn execute_shell(state: &AppState, command: &str) -> String {
    let docker = state.docker.read().await;
    let docker = match docker.as_ref() {
        Some(d) => d,
        None => {
            return "Error: Docker is not available. Install Docker to enable tool execution."
                .into()
        }
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
                format!(
                    "Command timed out after {} seconds",
                    opts.timeout_seconds
                )
            } else {
                format!(
                    "Exit code: {}\nStdout:\n{}\nStderr:\n{}",
                    result.exit_code, result.stdout, result.stderr
                )
            }
        }
        Err(e) => format!("Sandbox error: {}", e),
    }
}

fn inject_memories(messages: &mut Vec<serde_json::Value>, memories: &[Episode]) {
    if memories.is_empty() {
        return;
    }

    let memory_text = memories
        .iter()
        .map(|ep| {
            format!(
                "[Memory from {}] {}: {}",
                ep.created_at, ep.event_type, ep.summary
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let injection = format!(
        "\n\n--- Relevant memories from past tasks ---\n{}\n--- End memories ---",
        memory_text
    );

    // Find existing system message and append, or create one
    if let Some(system_msg) = messages.iter_mut().find(|m| m["role"] == "system") {
        if let Some(content) = system_msg["content"].as_str() {
            system_msg["content"] =
                serde_json::Value::String(format!("{}{}", content, injection));
        }
    } else {
        messages.insert(
            0,
            serde_json::json!({
                "role": "system",
                "content": injection
            }),
        );
    }
}
