use axum::extract::State;
use axum::Json;
use std::sync::Arc;

use crate::state::AppState;

pub async fn health_check(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let docker = state.docker.read().await;
    Json(serde_json::json!({
        "status": "ok",
        "version": "0.9.0",
        "docker_available": docker.is_some()
    }))
}
