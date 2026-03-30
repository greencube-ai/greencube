use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;

use crate::state::AppState;

pub mod agents;
pub mod brain;
pub mod completions;
pub mod health;

pub fn create_router(state: Arc<AppState>) -> Router {
    // SECURITY: Restrict CORS to localhost origins only.
    // This prevents any external website from making requests to the API.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            if let Ok(s) = origin.to_str() {
                s.starts_with("http://localhost")
                    || s.starts_with("http://127.0.0.1")
                    || s.starts_with("https://tauri.localhost")
                    || s.starts_with("tauri://localhost")
            } else {
                false
            }
        }))
        .allow_methods(AllowMethods::any())
        .allow_headers(AllowHeaders::any());

    Router::new()
        // Health
        .route("/health", get(health::health_check))
        // Agents
        .route("/v1/agents", get(agents::list_agents))
        .route("/v1/agents", post(agents::create_agent))
        .route("/v1/agents/{id}", get(agents::get_agent))
        // OpenAI-compatible
        .route(
            "/v1/chat/completions",
            post(completions::chat_completions),
        )
        // Memory
        .route("/v1/agents/{id}/episodes", get(agents::get_episodes))
        // Audit
        .route("/v1/agents/{id}/audit", get(agents::get_audit_log))
        // Human-readable brain endpoints (plain text, curl-friendly)
        .route("/brain", get(brain::brain))
        .route("/brain/{index}", get(brain::brain_by_index))
        .route("/status", get(brain::status))
        .route("/log", get(brain::log))
        .layer(cors)
        .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024)) // 10MB
        .with_state(state)
}
