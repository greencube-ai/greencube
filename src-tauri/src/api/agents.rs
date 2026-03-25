use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;

use crate::identity::registry;
use crate::identity::AgentResponse;
use crate::memory::episodic;
use crate::memory::Episode;
use crate::permissions::audit;
use crate::permissions::audit::AuditEntry;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
    pub system_prompt: Option<String>,
    pub tools_allowed: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct EpisodeQuery {
    pub limit: Option<i64>,
    pub task_id: Option<String>,
}

#[derive(Deserialize)]
pub struct AuditQuery {
    pub limit: Option<i64>,
}

pub async fn list_agents(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    match registry::list_agents(&db) {
        Ok(agents) => {
            let responses: Vec<AgentResponse> = agents.iter().map(|a| a.to_response()).collect();
            Ok(Json(serde_json::json!({ "agents": responses })))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

pub async fn create_agent(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateAgentRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    if body.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "name is required" })),
        ));
    }

    let tools = body
        .tools_allowed
        .unwrap_or_else(|| vec!["shell".into(), "read_file".into(), "write_file".into()]);
    let system_prompt = body.system_prompt.unwrap_or_default();

    let db = state.db.lock().await;
    match registry::create_agent(&db, &body.name, &system_prompt, &tools) {
        Ok(agent) => Ok((
            StatusCode::CREATED,
            Json(serde_json::json!(agent.to_response())),
        )),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("already exists") {
                Err((
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({ "error": msg })),
                ))
            } else if msg.contains("invalid tool") {
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": msg })),
                ))
            } else {
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": msg })),
                ))
            }
        }
    }
}

pub async fn get_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    match registry::get_agent(&db, &id) {
        Ok(Some(agent)) => Ok(Json(serde_json::json!(agent.to_response()))),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "agent not found" })),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

pub async fn get_episodes(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<EpisodeQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    let limit = query.limit.unwrap_or(50);
    let task_id = query.task_id.as_deref();

    match episodic::get_episodes(&db, &id, limit, task_id) {
        Ok(episodes) => Ok(Json(serde_json::json!({ "episodes": episodes }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

pub async fn get_audit_log(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    let limit = query.limit.unwrap_or(50);

    match audit::get_audit_log(&db, &id, limit) {
        Ok(entries) => Ok(Json(serde_json::json!({ "entries": entries }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}
