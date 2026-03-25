use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;

use crate::state::AppState;

pub mod agents;
pub mod completions;
pub mod health;

#[cfg(test)]
mod integration_test;

pub fn create_router(state: Arc<AppState>) -> Router {
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
        .layer(CorsLayer::permissive())
        .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024)) // 10MB
        .with_state(state)
}
