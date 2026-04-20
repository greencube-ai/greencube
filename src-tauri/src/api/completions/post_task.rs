use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use axum::Json;
use tauri::Emitter;

use crate::competence;
use crate::identity::registry;
use crate::memory::episodic;
use crate::memory::Episode;
use crate::permissions::audit;
use crate::permissions::audit::AuditEntry;
use crate::state::AppState;
use crate::task_outcome::TaskOutcome;

use super::helpers::{emit_refresh, emit_status};

/// All post-task side effects: episodes, drives, mood, reflection, self-verify, etc.
/// Called from both non-streaming (via finish_task) and streaming paths.
pub(super) async fn run_post_task(
    state: Arc<AppState>,
    agent_id: &str,
    task_id: &str,
    success: bool,
    cost_cents: i64,
    provider: Option<&crate::providers::Provider>,
    messages: Option<&[serde_json::Value]>,
    outcome: TaskOutcome,
) {
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

    // First-task cost toast: show once ever per agent
    {
        let db = state.db.lock().await;
        let first_toast_key = format!("first_task_toast_{}", agent_id);
        let shown: bool = db.query_row(
            "SELECT COUNT(*) FROM config_store WHERE key = ?1", rusqlite::params![first_toast_key], |r| r.get::<_, i64>(0),
        ).unwrap_or(0) > 0;
        if !shown {
            let _ = db.execute(
                "INSERT INTO config_store (key, value) VALUES (?1, '1') ON CONFLICT(key) DO UPDATE SET value = '1'",
                rusqlite::params![first_toast_key],
            );
            // Estimate: reflection ~500 + self-verify ~100 = ~600 tokens overhead
            let est_cost = 600.0 / 1000.0 * 0.01;
            if let Some(handle) = &state.app_handle {
                let _ = handle.emit("toast", serde_json::json!({
                    "type": "learning",
                    "message": format!("greencube used ~600 extra tokens (~${:.3}) for learning. this helps your agent improve over time.", est_cost)
                }));
            }
        }
    }

    // Record growth metrics
    {
        let db = state.db.lock().await;
        let _ = crate::metrics::record_metric(&db, agent_id);
    }

    // Background features — always on, no modes
    let config = state.config.read().await;
    let reflection_enabled = config.llm.self_reflection_enabled;
    drop(config);

    if let (Some(provider), Some(msgs)) = (provider, messages) {
        let (total_tasks, active_goal_count) = {
            let db = state.db.lock().await;
            let total = registry::get_agent(&db, agent_id).ok().flatten().map(|a| a.total_tasks).unwrap_or(0);
            let goals = crate::goals::count_active_goals(&db, agent_id).unwrap_or(0);
            (total, goals)
        };

        // Reflection: every task for first 5 (bootstrap), then every 3rd
        let should_reflect = reflection_enabled && msgs.len() >= 2
            && (total_tasks <= 5 || total_tasks % 3 == 0);
        if should_reflect {
            crate::reflection::spawn_reflection(
                state.clone(), agent_id.to_string(), provider.clone(),
                msgs.to_vec(), task_id.to_string(), success,
            );
        }

        // Reflection on failed tasks always runs
        if !success && !should_reflect && reflection_enabled && msgs.len() >= 2 {
            crate::reflection::spawn_reflection(
                state.clone(), agent_id.to_string(), provider.clone(),
                msgs.to_vec(), task_id.to_string(), success,
            );
        }

        if success {
            // Disabled — creature feature, not on signal path. File kept for future cleanup sweep.
            // crate::profile::maybe_regenerate(
            //     state.clone(), agent_id.to_string(), provider.clone(), total_tasks,
            // );

            // Disabled — creature feature, not on signal path. File kept for future cleanup sweep.
            // crate::goals::maybe_generate_goals(
            //     state.clone(), agent_id.to_string(), provider.clone(),
            //     total_tasks, active_goal_count,
            // );
        }

    }

    // Grounded Judge — evaluate task outcome, update competence
    let verdict = crate::self_verify::judge_task(&outcome);
    if verdict.delta != 0.0 {
        let db = state.db.lock().await;
        let domain = competence::get_most_recent_domain(&db, agent_id)
            .ok().flatten().unwrap_or_else(|| "general".into());
        let is_success = verdict.delta > 0.0;
        let _ = competence::update_competence(&db, agent_id, &domain, is_success, None);
    }
    // Log verdict as episode — full TaskOutcome snapshot for audit trail
    {
        let db = state.db.lock().await;
        let _ = episodic::insert_episode(&db, &Episode {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            event_type: "judge_verdict".into(),
            summary: format!("Judge: delta={:+.2}, {}", verdict.delta, verdict.reason),
            raw_data: Some(serde_json::json!({
                "delta": verdict.delta,
                "reason": verdict.reason,
                "signals_used": verdict.signals_used,
                "outcome": {
                    "tool_call_count": outcome.tool_call_count,
                    "tool_error_count": outcome.tool_error_count,
                    "llm_rounds": outcome.llm_rounds,
                    "duration_ms": outcome.duration_ms,
                    "cost_cents": outcome.cost_cents,
                    "success": outcome.success
                }
            }).to_string()),
            task_id: Some(task_id.into()),
            outcome: Some(if verdict.delta > 0.0 { "success" } else if verdict.delta < 0.0 { "failure" } else { "neutral" }.into()),
            tokens_used: 0,
            cost_cents: 0,
        });
    }
}

/// Non-streaming finish: run all post-task side effects, then build HTTP Response.
pub(super) async fn finish_task(
    state: Arc<AppState>,
    agent_id: &str,
    task_id: &str,
    success: bool,
    cost_cents: i64,
    response: serde_json::Value,
    provider: Option<&crate::providers::Provider>,
    messages: Option<&[serde_json::Value]>,
    outcome: TaskOutcome,
) -> Response {
    run_post_task(state.clone(), agent_id, task_id, success, cost_cents, provider, messages, outcome).await;

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
