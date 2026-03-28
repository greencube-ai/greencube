use axum::extract::State;
use axum::Json;
use std::sync::Arc;

use crate::state::AppState;

pub async fn health_check(State(_state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": "1.0.0"
    }))
}
