use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::identity::registry;
use crate::identity::Agent;
use crate::permissions::audit;
use crate::permissions::audit::AuditEntry;
use crate::state::AppState;

use super::helpers::emit_refresh;

/// 2b. SELF-DELEGATION: If agent is weak in the task's domain and has a specialist, reroute.
///
/// Returns `Some(Response)` if delegation fires and the specialist responds successfully —
/// caller should return that Response immediately. Returns `None` if delegation does not
/// fire or the specialist call fails (caller should proceed with normal flow).
pub(super) async fn try_self_delegate(
    state: &Arc<AppState>,
    agent: &Agent,
    body: &serde_json::Value,
) -> Option<Response> {
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
            emit_refresh(state);

            // Forward the request to the specialist by sending it as a message
            let user_content = body["messages"]
                .as_array()
                .and_then(|msgs| msgs.iter().rev().find(|m| m["role"] == "user"))
                .and_then(|m| m["content"].as_str())
                .unwrap_or("")
                .to_string();

            match crate::agent_messages::send_message(state, &agent.id, &specialist.name, &user_content, 0).await {
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
                    return Some(Json(resp_body).into_response());
                }
                Err(e) => {
                    tracing::warn!("Delegation failed, proceeding with parent: {}", e);
                    // Fall through to normal processing
                }
            }
        }
    }

    None
}
