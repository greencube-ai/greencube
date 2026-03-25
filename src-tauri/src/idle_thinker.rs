use std::sync::Arc;
use rusqlite::{params, Connection};
use tauri::Emitter;

use crate::goals;
use crate::knowledge;
use crate::notifications;
use crate::providers;
use crate::state::AppState;

const MAX_IDLE_NOTIFICATIONS_PER_DAY: i64 = 3;

/// Main idle thinking loop. Spawned from main.rs.
pub async fn run_idle_thinker(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        interval.tick().await;

        // Check if Alive Mode is enabled AND idle thinking is enabled
        let config = state.config.read().await;
        if !config.ui.alive_mode || !config.idle.idle_thinking_enabled {
            continue;
        }
        let idle_minutes = config.idle.idle_minutes_before_think;
        let max_daily_cycles = config.idle.max_daily_idle_cycles;
        drop(config);

        // Get all idle agents
        let agents = {
            let db = state.db.lock().await;
            match crate::identity::registry::list_agents(&db) {
                Ok(a) => a,
                Err(_) => continue,
            }
        };

        for agent in agents {
            if agent.status != "idle" { continue; }

            // Check if agent has been idle long enough
            let idle_duration = chrono::Utc::now()
                .signed_duration_since(
                    chrono::DateTime::parse_from_rfc3339(&agent.updated_at)
                        .unwrap_or_else(|_| chrono::Utc::now().into())
                );
            if idle_duration.num_minutes() < idle_minutes as i64 { continue; }

            // Check daily cycle count
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let today_key = format!("idle_cycles_{}_{}", agent.id, today);
            let cycle_count = {
                let db = state.db.lock().await;
                get_config_counter(&db, &today_key)
            };
            if cycle_count >= max_daily_cycles as i64 { continue; }

            // Get provider
            let provider = {
                let db = state.db.lock().await;
                match providers::get_provider_for_agent(&db, &agent) {
                    Ok(p) => p,
                    Err(_) => continue,
                }
            };

            // Gather context (brief DB lock, then release before LLM call)
            let (knowledge_entries, context, active_goals) = {
                let db = state.db.lock().await;
                let knowledge = knowledge::list_knowledge(&db, &agent.id, 10).unwrap_or_default();
                let ctx = crate::context::get_context(&db, &agent.id).unwrap_or_default();
                let goals_list = goals::list_goals(&db, &agent.id, Some("active")).unwrap_or_default();
                (knowledge, ctx, goals_list)
            };
            // DB lock released here!

            // Build thinking prompt
            let knowledge_text = if knowledge_entries.is_empty() {
                "None yet.".to_string()
            } else {
                knowledge_entries.iter()
                    .map(|k| format!("- [{}] {}", k.category, k.content))
                    .collect::<Vec<_>>().join("\n")
            };

            let goals_text = if active_goals.is_empty() {
                "No goals set.".to_string()
            } else {
                active_goals.iter().map(|g| format!("- {}", g.content)).collect::<Vec<_>>().join("\n")
            };

            let time_context = crate::time_sense::time_context_for_agent(&agent.updated_at);

            let prompt = format!(
                r#"{}

You are currently idle. No one has asked you anything.
Review what you know and think independently.

Your knowledge base:
{}

Your current context:
{}

Your goals:
{}

Think about:
1. Do any of your knowledge entries contradict each other?
2. Are there gaps in what you know that would be useful to fill?
3. Do you see connections between things you've learned?
4. Is there anything you should tell your human about?

Format your thoughts:
[insight] A new understanding or connection
[question] Something you'd like to explore
[gap] Something missing from your knowledge
[notify] Something important to tell the human (this will send a notification)
[spawn] domain_name — if you have 20+ tasks and struggle in a domain (<55% success, 8+ tasks), spawn a specialist

If you have nothing to think about, respond: IDLE
Max 3 thoughts per cycle."#,
                time_context,
                knowledge_text,
                if context.is_empty() { "None set." } else { &context },
                goals_text,
            );

            // Make LLM call (no DB lock held!)
            let client = reqwest::Client::new();
            let response = match client.post(format!("{}/chat/completions", provider.api_base_url))
                .header("Authorization", format!("Bearer {}", provider.api_key))
                .json(&serde_json::json!({
                    "model": provider.default_model,
                    "messages": [
                        {"role": "system", "content": crate::commandments::AGENT_COMMANDMENTS},
                        {"role": "user", "content": prompt}
                    ],
                    "max_tokens": 400,
                    "temperature": 0.7,
                }))
                .timeout(std::time::Duration::from_secs(30))
                .send().await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Idle thought LLM call failed for {}: {}", agent.id, e);
                    continue;
                }
            };

            if !response.status().is_success() { continue; }

            let body: serde_json::Value = match response.json().await {
                Ok(b) => b,
                Err(_) => continue,
            };

            let content = body["choices"][0]["message"]["content"].as_str().unwrap_or("IDLE");
            if content.trim() == "IDLE" {
                // Still count the cycle
                let db = state.db.lock().await;
                increment_config_counter(&db, &today_key);
                continue;
            }

            // Parse thoughts and store (brief DB lock)
            let notification_count_today = {
                let db = state.db.lock().await;
                notifications::count_idle_notifications_today(&db, &agent.id).unwrap_or(0)
            };

            let mut spawn_request: Option<String> = None;
            {
                let db = state.db.lock().await;
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed == "IDLE" { continue; }

                    if let Some(text) = trimmed.strip_prefix("[notify]") {
                        let text = text.trim();
                        if !text.is_empty() {
                            if notification_count_today < MAX_IDLE_NOTIFICATIONS_PER_DAY {
                                // Create real notification
                                let _ = notifications::create_notification(&db, &agent.id, text, "insight", "idle_thought");
                                if let Some(handle) = &state.app_handle {
                                    let _ = handle.emit("notification-new", serde_json::json!({"agent_id": &agent.id}));
                                }
                            } else {
                                // Store as regular insight instead (daily limit reached)
                                let _ = insert_thought(&db, &agent.id, text, "insight");
                            }
                        }
                    } else if let Some(text) = trimmed.strip_prefix("[insight]") {
                        let text = text.trim();
                        if !text.is_empty() {
                            let _ = insert_thought(&db, &agent.id, text, "insight");
                            let _ = knowledge::insert_knowledge(&db, &agent.id, text, "fact", None);
                        }
                    } else if let Some(text) = trimmed.strip_prefix("[question]") {
                        let text = text.trim();
                        if !text.is_empty() {
                            let _ = insert_thought(&db, &agent.id, text, "question");
                        }
                    } else if let Some(text) = trimmed.strip_prefix("[gap]") {
                        let text = text.trim();
                        if !text.is_empty() {
                            let _ = insert_thought(&db, &agent.id, text, "gap");
                        }
                    } else if let Some(text) = trimmed.strip_prefix("[connection]") {
                        let text = text.trim();
                        if !text.is_empty() {
                            let _ = insert_thought(&db, &agent.id, text, "connection");
                            let _ = knowledge::insert_knowledge(&db, &agent.id, text, "fact", None);
                        }
                    } else if let Some(text) = trimmed.strip_prefix("[spawn]") {
                        let domain = text.trim().to_string();
                        if !domain.is_empty() && crate::spawn::can_spawn(&db, &agent.id) {
                            let _ = insert_thought(&db, &agent.id, &format!("Initiating spawn for domain: {}", domain), "spawn");
                            spawn_request = Some(domain);
                        }
                    }
                }
                increment_config_counter(&db, &today_key);
            }
            // DB lock released. Handle spawn outside the lock.
            if let Some(domain) = spawn_request {
                match crate::spawn::execute_spawn(&state, &agent.id, &domain).await {
                    Ok(child_name) => tracing::info!("Idle thinker spawned {} for agent {}", child_name, agent.id),
                    Err(e) => tracing::warn!("Idle thinker spawn failed for {}: {}", domain, e),
                }
            }

            if let Some(handle) = &state.app_handle {
                let _ = handle.emit("activity-refresh", ());
            }
            tracing::info!("Idle thought cycle completed for agent {}", agent.id);
        }
    }
}

fn insert_thought(conn: &Connection, agent_id: &str, content: &str, thought_type: &str) -> anyhow::Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO idle_thoughts (id, agent_id, content, thought_type, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, agent_id, content, thought_type, now],
    )?;
    Ok(())
}

pub fn get_recent_thoughts(conn: &Connection, agent_id: &str, limit: i64) -> anyhow::Result<Vec<(String, String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, content, thought_type, created_at FROM idle_thoughts WHERE agent_id = ?1 ORDER BY created_at DESC LIMIT ?2"
    )?;
    let thoughts = stmt.query_map(params![agent_id, limit], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?))
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(thoughts)
}

fn get_config_counter(conn: &Connection, key: &str) -> i64 {
    conn.query_row(
        "SELECT CAST(value AS INTEGER) FROM config_store WHERE key = ?1",
        params![key],
        |row| row.get(0),
    ).unwrap_or(0)
}

fn increment_config_counter(conn: &Connection, key: &str) {
    let current = get_config_counter(conn, key);
    let _ = conn.execute(
        "INSERT INTO config_store (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![key, (current + 1).to_string()],
    );
}
