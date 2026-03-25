use crate::config::{self, AppConfig};
use crate::errors::GreenCubeError;
use crate::identity::registry;
use crate::identity::AgentResponse;
use crate::memory::episodic;
use crate::memory::Episode;
use crate::permissions::audit;
use crate::permissions::audit::AuditEntry;
use crate::providers;
use crate::providers::Provider;
use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

type Result<T> = std::result::Result<T, GreenCubeError>;

#[tauri::command]
pub async fn get_agents(state: State<'_, Arc<AppState>>) -> Result<Vec<AgentResponse>> {
    let db = state.db.lock().await;
    let agents =
        registry::list_agents(&db).map_err(|e| GreenCubeError::Internal(e.to_string()))?;
    Ok(agents.iter().map(|a| a.to_response()).collect())
}

#[tauri::command]
pub async fn get_agent(id: String, state: State<'_, Arc<AppState>>) -> Result<AgentResponse> {
    let db = state.db.lock().await;
    let agent = registry::get_agent(&db, &id)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?
        .ok_or_else(|| GreenCubeError::AgentNotFound(id))?;
    Ok(agent.to_response())
}

#[tauri::command]
pub async fn create_agent(
    name: String,
    system_prompt: String,
    tools_allowed: Vec<String>,
    provider_id: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<AgentResponse> {
    let db = state.db.lock().await;
    let agent = registry::create_agent_with_provider(
        &db, &name, &system_prompt, &tools_allowed, provider_id.as_deref()
    ).map_err(|e| GreenCubeError::Validation(e.to_string()))?;
    Ok(agent.to_response())
}

#[tauri::command]
pub async fn get_episodes(
    agent_id: String,
    limit: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Episode>> {
    let db = state.db.lock().await;
    let episodes = episodic::get_episodes(&db, &agent_id, limit.unwrap_or(50), None)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;
    Ok(episodes)
}

#[tauri::command]
pub async fn get_audit_log(
    agent_id: String,
    limit: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<AuditEntry>> {
    let db = state.db.lock().await;
    let entries = audit::get_audit_log(&db, &agent_id, limit.unwrap_or(50))
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;
    Ok(entries)
}

#[tauri::command]
pub async fn get_activity_feed(
    limit: Option<i64>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<AuditEntry>> {
    let db = state.db.lock().await;
    let entries = audit::get_recent_activity(&db, limit.unwrap_or(50))
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;
    Ok(entries)
}

#[tauri::command]
pub async fn get_config(state: State<'_, Arc<AppState>>) -> Result<AppConfig> {
    let config = state.config.read().await;
    Ok(config.clone())
}

#[tauri::command]
pub async fn save_config(
    config: AppConfig,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    config::save_config(&config).map_err(|e| GreenCubeError::Config(e.to_string()))?;
    let mut current = state.config.write().await;
    *current = config;
    Ok(())
}

#[tauri::command]
pub async fn get_docker_status(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value> {
    let docker = state.docker.read().await;
    Ok(serde_json::json!({
        "available": docker.is_some()
    }))
}

#[tauri::command]
pub async fn get_server_info(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "port": state.actual_port,
        "host": "127.0.0.1"
    }))
}

#[tauri::command]
pub async fn reset_app(_state: State<'_, Arc<AppState>>) -> Result<()> {
    let data_dir = crate::config::config_dir();
    if data_dir.exists() {
        std::fs::remove_dir_all(&data_dir)
            .map_err(|e| GreenCubeError::Internal(format!("Failed to delete data: {}", e)))?;
    }
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| GreenCubeError::Internal(format!("Failed to recreate dir: {}", e)))?;
    let default_config = AppConfig::default();
    crate::config::save_config(&default_config)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;
    Ok(())
}

// ─── Knowledge Commands ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_knowledge(agent_id: String, limit: Option<i64>, state: State<'_, Arc<AppState>>) -> Result<Vec<crate::knowledge::KnowledgeEntry>> {
    let db = state.db.lock().await;
    crate::knowledge::list_knowledge(&db, &agent_id, limit.unwrap_or(50))
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

// ─── Context Commands ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_agent_context(agent_id: String, state: State<'_, Arc<AppState>>) -> Result<String> {
    let db = state.db.lock().await;
    crate::context::get_context(&db, &agent_id)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn set_agent_context(agent_id: String, content: String, state: State<'_, Arc<AppState>>) -> Result<()> {
    let db = state.db.lock().await;
    crate::context::set_context(&db, &agent_id, &content)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

// ─── Provider Commands ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_providers(state: State<'_, Arc<AppState>>) -> Result<Vec<Provider>> {
    let db = state.db.lock().await;
    providers::list_providers(&db).map_err(|e| GreenCubeError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn create_provider(
    name: String,
    api_base_url: String,
    api_key: String,
    default_model: String,
    provider_type: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Provider> {
    let db = state.db.lock().await;
    providers::create_provider(&db, &name, &api_base_url, &api_key, &default_model, &provider_type)
        .map_err(|e| GreenCubeError::Validation(e.to_string()))
}

#[tauri::command]
pub async fn update_provider(
    id: String,
    name: String,
    api_base_url: String,
    api_key: String,
    default_model: String,
    provider_type: String,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let db = state.db.lock().await;
    providers::update_provider(&db, &id, &name, &api_base_url, &api_key, &default_model, &provider_type)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn delete_provider(id: String, state: State<'_, Arc<AppState>>) -> Result<()> {
    let db = state.db.lock().await;
    providers::delete_provider(&db, &id)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}
