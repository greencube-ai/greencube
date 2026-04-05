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
use tauri::{Manager, State};

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

    // Sync API key to ALL providers in the database
    if !config.llm.api_key.is_empty() {
        let db = state.db.lock().await;
        let _ = db.execute(
            "UPDATE providers SET api_key = ?1, api_base_url = ?2, default_model = ?3",
            rusqlite::params![config.llm.api_key, config.llm.api_base_url, config.llm.default_model],
        );
        tracing::info!("Synced API key from config to all providers");
    }

    let mut current = state.config.write().await;
    *current = config;
    Ok(())
}

#[tauri::command]
pub async fn get_docker_status(_state: State<'_, Arc<AppState>>) -> Result<serde_json::Value> {
    // Docker removed — tools run directly on host
    Ok(serde_json::json!({ "available": true }))
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
             DELETE FROM agent_goals;
             DELETE FROM agent_metrics;
             DELETE FROM agent_capabilities;
             DELETE FROM messages;
             DELETE FROM idle_thoughts;
             DELETE FROM journal_entries;
             DELETE FROM task_patterns;
             DELETE FROM agent_forks;
             DELETE FROM curiosities;
             DELETE FROM drives;
             DELETE FROM context_clusters;
             DELETE FROM relationships;
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

    // Thumbs down → lower competence + store specific correction
    if rating < 0 {
        let domain = crate::competence::get_most_recent_domain(&db, &agent_id).ok().flatten();
        if let Some(ref d) = domain {
            let _ = crate::competence::update_competence(&db, &agent_id, d, false, None);
            tracing::info!("Human thumbs-down: competence lowered for agent {} domain '{}'", agent_id, d);
        }

        // Look up what the task was actually about for a useful correction
        let task_summary = crate::memory::episodic::get_episodes(&db, &agent_id, 5, Some(&task_id))
            .unwrap_or_default()
            .into_iter()
            .find(|e| e.event_type == "task_start")
            .map(|e| e.summary.replace("Task started: ", ""))
            .unwrap_or_else(|| "unknown task".to_string());

        let domain_label = domain.as_deref().unwrap_or("general");
        let _ = crate::knowledge::insert_knowledge(
            &db, &agent_id,
            &format!("CORRECTION [{}]: User rejected response about '{}'. Do not repeat this approach.", domain_label, task_summary),
            "correction", Some(&task_id),
        );
    } else if rating > 0 {
        let domain = crate::competence::get_most_recent_domain(&db, &agent_id).ok().flatten();
        let domain_label = domain.as_deref().unwrap_or("general");
        let _ = crate::knowledge::insert_knowledge(
            &db, &agent_id,
            &format!("PRAISED [{}]: User approved this response.", domain_label),
            "praise", Some(&task_id),
        );
    }

    // Update relationship signals
    let _ = crate::relationships::record_signal(&db, &agent_id, "default_user", rating > 0);

    Ok(())
}

#[tauri::command]
pub async fn get_competence_map(agent_id: String, state: State<'_, Arc<AppState>>) -> Result<Vec<crate::competence::CompetenceEntry>> {
    let db = state.db.lock().await;
    crate::competence::get_competence_map(&db, &agent_id)
        .map_err(|e| GreenCubeError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn get_creature_status(agent_id: String, state: State<'_, Arc<AppState>>) -> Result<crate::creature_status::CreatureStatus> {
    let db = state.db.lock().await;
    Ok(crate::creature_status::get_creature_status(&db, &agent_id))
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

// ─── OpenClaw Integration ──────────────────────────────────────────────────

/// Read and parse ~/.openclaw/openclaw.json (or %USERPROFILE%\.openclaw\openclaw.json on Windows)
#[tauri::command]
pub async fn read_openclaw_config() -> Result<serde_json::Value> {
    let home = dirs::home_dir().ok_or_else(|| GreenCubeError::Internal("cannot find home directory".into()))?;
    let config_path = home.join(".openclaw").join("openclaw.json");
    if !config_path.exists() {
        return Err(GreenCubeError::Internal("not_found".into()));
    }
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| GreenCubeError::Internal(format!("failed to read openclaw config: {}", e)))?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| GreenCubeError::Internal(format!("failed to parse openclaw config: {}", e)))?;
    Ok(json)
}

/// Auto-configure OpenClaw to route through GreenCube.
/// Reads existing config, adds greencube provider, sets it as default, writes back.
#[tauri::command]
pub async fn configure_openclaw(port: u16, state: State<'_, Arc<AppState>>) -> Result<serde_json::Value> {
    let home = dirs::home_dir().ok_or_else(|| GreenCubeError::Internal("cannot find home directory".into()))?;
    let config_path = home.join(".openclaw").join("openclaw.json");
    if !config_path.exists() {
        return Err(GreenCubeError::Internal("not_found".into()));
    }

    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| GreenCubeError::Internal(format!("failed to read: {}", e)))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| GreenCubeError::Internal(format!("failed to parse: {}", e)))?;

    // Find existing API key and model from any existing provider
    let mut found_key = String::new();
    let mut found_model = "gpt-4o".to_string();
    if let Some(providers) = config.pointer("/models/providers").and_then(|p| p.as_object()) {
        for (_name, provider) in providers {
            if let Some(key) = provider["apiKey"].as_str() {
                if !key.is_empty() && key != "local" {
                    found_key = key.to_string();
                }
            }
            if let Some(models) = provider["models"].as_array() {
                if let Some(first) = models.first() {
                    if let Some(id) = first["id"].as_str() {
                        found_model = id.to_string();
                    }
                }
            }
        }
    }

    // Build greencube provider entry
    let gc_provider = serde_json::json!({
        "baseUrl": format!("http://localhost:{}/v1", port),
        "apiKey": found_key,
        "api": "openai-completions",
        "models": [{
            "id": found_model,
            "name": found_model,
            "reasoning": false,
            "input": ["text"],
            "contextWindow": 128000,
            "maxTokens": 16384
        }]
    });

    // Ensure models.providers exists and add greencube
    if config.pointer("/models/providers").is_none() {
        config["models"]["providers"] = serde_json::json!({});
    }
    config["models"]["providers"]["greencube"] = gc_provider;
    if config.pointer("/models/mode").is_none() {
        config["models"]["mode"] = serde_json::json!("merge");
    }

    // Set default model to greencube
    let gc_model = format!("greencube/{}", found_model);
    config["agents"]["defaults"]["model"]["primary"] = serde_json::json!(gc_model);
    config["agents"]["defaults"]["models"][&gc_model] = serde_json::json!({"alias": found_model});

    // Write back
    let pretty = serde_json::to_string_pretty(&config)
        .map_err(|e| GreenCubeError::Internal(format!("failed to serialize: {}", e)))?;
    std::fs::write(&config_path, &pretty)
        .map_err(|e| GreenCubeError::Internal(format!("failed to write: {}", e)))?;

    // Also save the found API key to GreenCube's own config
    if !found_key.is_empty() {
        let mut gc_config = state.config.write().await;
        gc_config.llm.api_key = found_key.clone();
        gc_config.llm.default_model = found_model.clone();
        let _ = config::save_config(&gc_config);
        drop(gc_config);

        // Sync to providers table via the existing save_config path
        let db = state.db.lock().await;
        // Update default provider or create one
        if let Ok(Some(p)) = providers::get_default_provider(&db) {
            let _ = providers::update_provider(&db, &p.id, &p.name, &p.api_base_url, &found_key, &found_model, &p.provider_type);
        } else {
            let _ = providers::create_provider(&db, "default", "https://api.openai.com/v1", &found_key, &found_model, "openai");
        }
    }

    Ok(serde_json::json!({"model": found_model, "key_found": !found_key.is_empty()}))
}

/// Run openclaw daemon restart
#[tauri::command]
pub async fn restart_openclaw() -> Result<String> {
    let shell = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
    let flag = if cfg!(target_os = "windows") { "/C" } else { "-c" };
    match tokio::process::Command::new(shell)
        .arg(flag)
        .arg("openclaw daemon restart")
        .output()
        .await
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                Ok(format!("{}{}", stdout, stderr))
            } else {
                Err(GreenCubeError::Internal(format!("openclaw restart failed: {}{}", stdout, stderr)))
            }
        }
        Err(e) => Err(GreenCubeError::Internal(format!("failed to run openclaw: {}", e))),
    }
}

/// Persist OPENAI_API_BASE env var across sessions
#[tauri::command]
pub async fn set_env_permanently(value: String) -> Result<String> {
    if cfg!(target_os = "windows") {
        // Windows: set user-level env var via PowerShell
        let script = format!(
            r#"[System.Environment]::SetEnvironmentVariable("OPENAI_API_BASE", "{}", "User")"#,
            value
        );
        let output = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .await
            .map_err(|e| GreenCubeError::Internal(format!("failed to run powershell: {}", e)))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GreenCubeError::Internal(format!("powershell failed: {}", stderr)));
        }
        Ok("windows".to_string())
    } else {
        // macOS/Linux: append to shell rc files if not already present
        let home = std::env::var("HOME")
            .map_err(|_| GreenCubeError::Internal("HOME not set".to_string()))?;
        let export_line = format!("export OPENAI_API_BASE=\"{}\"", value);
        let mut updated = Vec::new();

        for rc in &[".zshrc", ".bashrc"] {
            let path = std::path::PathBuf::from(&home).join(rc);
            if path.exists() {
                let contents = tokio::fs::read_to_string(&path).await
                    .map_err(|e| GreenCubeError::Internal(format!("failed to read {}: {}", rc, e)))?;
                if !contents.contains("OPENAI_API_BASE") {
                    let append = format!("\n# GreenCube proxy\n{}\n", export_line);
                    tokio::fs::write(&path, format!("{}{}", contents, append)).await
                        .map_err(|e| GreenCubeError::Internal(format!("failed to write {}: {}", rc, e)))?;
                    updated.push(rc.to_string());
                }
            }
        }
        Ok(format!("unix:{}", updated.join(",")))
    }
}

/// Minimize the main window to tray
#[tauri::command]
pub async fn minimize_to_tray(app: tauri::AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| GreenCubeError::Internal(format!("failed to hide window: {}", e)))?;
    }
    Ok(())
}
