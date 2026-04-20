use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::StreamExt;
use tauri::Emitter;

use crate::identity::registry;
use crate::memory::episodic;
use crate::memory::Episode;
use crate::permissions::audit;
use crate::permissions::audit::AuditEntry;
use crate::state::AppState;
use crate::task_outcome::TaskOutcome;

use super::helpers::{emit_refresh, emit_status, error_response, redact_secrets};
use super::post_task::run_post_task;

/// Stream an LLM response as SSE to the client. Used for simple chats (no tools).
pub(super) async fn stream_llm_response(
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
        // LLM returned JSON, not SSE — handle as non-streaming fallback.
        // Log LLM response episode, then delegate to run_post_task for all side effects.
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
                }
                // Streaming path is gated on !has_tools, so success=true is correct here.
                let msgs: Vec<serde_json::Value> = body["messages"].as_array().cloned().unwrap_or_default();
                run_post_task(state.clone(), agent_id, task_id, true, 0, Some(provider), Some(&msgs), TaskOutcome::new()).await;
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
                    // Streaming path is gated on !has_tools at line 557, so tools
                    // cannot be invoked here and there is nothing to fail. If
                    // streaming tool support is ever added, this must become a
                    // real success check like the non-streaming path.

                    // Log the LLM response episode (specific to streaming — run_post_task
                    // handles task_end episode, drives, mood, reflection, etc.)
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
                    }

                    run_post_task(
                        state_clone.clone(), &agent_id_owned, &task_id_owned,
                        true, 0, Some(&provider_clone), Some(&original_messages),
                        TaskOutcome::new(),
                    ).await;

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

        // Stream ended without [DONE] — abnormal termination, treat as failure.
        // Log partial LLM response if any, then delegate to run_post_task.
        if !accumulated_content.is_empty() {
            let db = state_clone.db.lock().await;
            let _ = episodic::insert_episode(&db, &Episode {
                id: uuid::Uuid::new_v4().to_string(), agent_id: agent_id_owned.clone(),
                created_at: chrono::Utc::now().to_rfc3339(), event_type: "llm_response".into(),
                summary: format!("LLM (partial): {}", accumulated_content.chars().take(200).collect::<String>()),
                raw_data: Some(accumulated_content), task_id: Some(task_id_owned.clone()),
                outcome: Some("partial".into()), tokens_used: 0, cost_cents: 0,
            });
        }

        run_post_task(
            state_clone.clone(), &agent_id_owned, &task_id_owned,
            false, 0, Some(&provider_clone), Some(&original_messages),
            TaskOutcome::new(),
        ).await;
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let mut response = Sse::new(stream).keep_alive(KeepAlive::default()).into_response();
    response.headers_mut().insert(
        "x-greencube-task-id",
        axum::http::HeaderValue::from_str(task_id).unwrap_or_else(|_| axum::http::HeaderValue::from_static("")),
    );
    response
}
