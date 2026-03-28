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
pub async fn reset_app(state: State<'_, Arc<AppState>>) -> Result<()> {
    // Drop all data by wiping tables (can't delete the db file while it's open on Windows)
    {
        let db = state.db.lock().await;
        db.execute_batch(
            "DELETE FROM audit_log;
             DELETE FROM episodes;
             DELETE FROM knowledge;
             DELETE FROM tool_results;
             DELETE FROM feedback_signals;
             DELETE FROM agent_context;
             DELETE FROM competence_map;
             DELETE FROM notifications;
             DELETE FROM agent_lineage;
             DELETE FROM response_ratings;
             DELETE FROM token_usage;
             DELETE FROM task_queue;
             DELETE FROM goals;
             DELETE FROM growth_metrics;
             DELETE FROM capabilities;
             DELETE FROM messages;
             DELETE FROM agents;
             DELETE FROM providers;
             DELETE FROM config_store;"
        ).map_err(|e| GreenCubeError::Internal(format!("Failed to clear data: {}", e)))?;
    }

    // Reset config to defaults (triggers onboarding)
    let default_config = AppConfig::default();
    crate::config::save_config(&default_config)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;

    // Update in-memory config
    let mut config = state.config.write().await;
    *config = default_config;

    Ok(())
}

// ─── Lineage Commands ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_agent_lineage(agent_id: String, state: State<'_, Arc<AppState>>) -> Result<serde_json::Value> {
    let db = state.db.lock().await;
    let lineage = crate::spawn::get_lineage(&db, &agent_id);
    Ok(serde_json::json!({
        "parent": lineage.parent.map(|(id, name, domain)| serde_json::json!({"id": id, "name": name, "domain": domain})),
        "children": lineage.children.iter().map(|(id, name, domain, count)| {
            serde_json::json!({"id": id, "name": name, "domain": domain, "knowledge_transferred": count})
        }).collect::<Vec<_>>(),
    }))
}

// ─── Debug Commands ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn debug_spawn(agent_id: String, domain: String, state: State<'_, Arc<AppState>>) -> Result<String> {
    // Debug: create a specialist without competence checks. Remove before real launch.
    match crate::spawn::debug_force_spawn(&state, &agent_id, &domain).await {
        Ok(name) => Ok(name),
        Err(e) => Err(GreenCubeError::Internal(e.to_string())),
    }
}

// ─── Rating Commands ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn rate_response(agent_id: String, task_id: String, rating: i32, state: State<'_, Arc<AppState>>) -> Result<()> {
    let db = state.db.lock().await;
    crate::ratings::rate_response(&db, &agent_id, &task_id, rating)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;

    // Thumbs down → lower competence in the agent's most recent domain
    if rating < 0 {
        if let Ok(Some(domain)) = crate::competence::get_most_recent_domain(&db, &agent_id) {
            let _ = crate::competence::update_competence(&db, &agent_id, &domain, false, None);
            tracing::info!("Human thumbs-down: competence lowered for agent {} domain '{}'", agent_id, domain);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn get_competence_map(agent_id: String, state: State<'_, Arc<AppState>>) -> Result<Vec<crate::competence::CompetenceEntry>> {
    let db = state.db.lock().await;
    crate::competence::get_competence_map(&db, &agent_id)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn get_approval_rate(agent_id: String, state: State<'_, Arc<AppState>>) -> Result<f64> {
    let db = state.db.lock().await;
    crate::ratings::get_approval_rate(&db, &agent_id, 30)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

// ─── Token Usage Commands ────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_token_usage_today(agent_id: String, state: State<'_, Arc<AppState>>) -> Result<serde_json::Value> {
    let db = state.db.lock().await;
    let summary = crate::token_usage::get_usage_today(&db, &agent_id)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;
    Ok(serde_json::json!({
        "task_tokens": summary.task_tokens,
        "background_tokens": summary.background_tokens,
        "breakdown": summary.breakdown,
    }))
}

// ─── Message Commands ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_agent_messages(agent_id: String, limit: Option<i64>, state: State<'_, Arc<AppState>>) -> Result<Vec<crate::agent_messages::AgentMessage>> {
    let db = state.db.lock().await;
    crate::agent_messages::get_messages(&db, &agent_id, limit.unwrap_or(50))
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

// ─── Goal Commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_goals(agent_id: String, status: Option<String>, state: State<'_, Arc<AppState>>) -> Result<Vec<crate::goals::Goal>> {
    let db = state.db.lock().await;
    crate::goals::list_goals(&db, &agent_id, status.as_deref())
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

// ─── Metrics Commands ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_metrics(agent_id: String, days: Option<i64>, state: State<'_, Arc<AppState>>) -> Result<Vec<crate::metrics::MetricSnapshot>> {
    let db = state.db.lock().await;
    crate::metrics::get_metrics(&db, &agent_id, days.unwrap_or(30))
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

// ─── Idle Thought Commands ──────────────────────────────────────────────────

#[tauri::command]
pub async fn get_idle_thoughts(agent_id: String, limit: Option<i64>, state: State<'_, Arc<AppState>>) -> Result<serde_json::Value> {
    let db = state.db.lock().await;
    let thoughts = crate::idle_thinker::get_recent_thoughts(&db, &agent_id, limit.unwrap_or(20))
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;
    Ok(serde_json::json!(thoughts.iter().map(|(id, content, ttype, created)| {
        serde_json::json!({"id": id, "content": content, "thought_type": ttype, "created_at": created})
    }).collect::<Vec<_>>()))
}

// ─── Capability Commands ────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_capabilities(agent_id: String, state: State<'_, Arc<AppState>>) -> Result<Vec<crate::capabilities::Capability>> {
    let db = state.db.lock().await;
    crate::capabilities::list_capabilities(&db, &agent_id)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn search_capabilities(query: String, state: State<'_, Arc<AppState>>) -> Result<serde_json::Value> {
    let db = state.db.lock().await;
    let results = crate::capabilities::search_capabilities(&db, &query)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))?;
    Ok(serde_json::json!(results.iter().map(|(agent_id, agent_name, cap, conf)| {
        serde_json::json!({"agent_id": agent_id, "agent_name": agent_name, "capability": cap, "confidence": conf})
    }).collect::<Vec<_>>()))
}

// ─── Notification Commands ───────────────────────────────────────────────────

#[tauri::command]
pub async fn get_unread_count(state: State<'_, Arc<AppState>>) -> Result<i64> {
    let db = state.db.lock().await;
    crate::notifications::get_unread_count(&db)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn get_notifications(unread_only: bool, limit: Option<i64>, state: State<'_, Arc<AppState>>) -> Result<Vec<crate::notifications::Notification>> {
    let db = state.db.lock().await;
    crate::notifications::get_notifications(&db, unread_only, limit.unwrap_or(50))
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn mark_notification_read(id: String, state: State<'_, Arc<AppState>>) -> Result<()> {
    let db = state.db.lock().await;
    crate::notifications::mark_read(&db, &id)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn dismiss_all_notifications(state: State<'_, Arc<AppState>>) -> Result<()> {
    let db = state.db.lock().await;
    crate::notifications::dismiss_all(&db)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
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
