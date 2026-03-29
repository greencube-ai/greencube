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

        // Check if idle thinking is enabled
        let config = state.config.read().await;
        if !config.idle.idle_thinking_enabled {
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

            // Check for urgency flag — react within 60s to important events
            let urgent_key = format!("urgent_think_{}", agent.id);
            let is_urgent = {
                let db = state.db.lock().await;
                let urgent = get_config_counter(&db, &urgent_key) > 0;
                if urgent {
                    let _ = db.execute("DELETE FROM config_store WHERE key = ?1", params![urgent_key]);
                }
                urgent
            };

            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

            // Urgent and regular cycles have separate daily budgets
            let today_key;
            if is_urgent {
                today_key = format!("urgent_cycles_{}_{}", agent.id, today);
                let urgent_count = {
                    let db = state.db.lock().await;
                    get_config_counter(&db, &today_key)
                };
                if urgent_count >= 5 { continue; } // urgent budget: 5/day
            } else {
                // Normal path: check if agent has been idle long enough
                let idle_duration = chrono::Utc::now()
                    .signed_duration_since(
                        chrono::DateTime::parse_from_rfc3339(&agent.updated_at)
                            .unwrap_or_else(|_| chrono::Utc::now().into())
                    );
                if idle_duration.num_minutes() < idle_minutes as i64 { continue; }

                today_key = format!("idle_cycles_{}_{}", agent.id, today);
                let cycle_count = {
                    let db = state.db.lock().await;
                    get_config_counter(&db, &today_key)
                };
                if cycle_count >= max_daily_cycles as i64 { continue; } // regular budget: 10/day
            }

            // Budget check: skip if daily background token budget exceeded
            {
                let db = state.db.lock().await;
                let budget = state.config.read().await.cost.daily_background_token_budget;
                if !crate::token_usage::has_budget_remaining(&db, &agent.id, 400, budget).unwrap_or(false) {
                    tracing::info!("Budget exceeded, skipping idle think for agent {}", agent.id);
                    continue;
                }
            }

            // Get provider
            let provider = {
                let db = state.db.lock().await;
                match providers::get_provider_for_agent(&db, &agent) {
                    Ok(p) => p,
                    Err(_) => continue,
                }
            };

            // Gather context (brief DB lock, then release before LLM call)
            let (knowledge_entries, context, active_goals, recent_episodes, notifs_remaining) = {
                let db = state.db.lock().await;
                let knowledge = knowledge::list_knowledge(&db, &agent.id, 10).unwrap_or_default();
                let ctx = crate::context::get_context(&db, &agent.id).unwrap_or_default();
                let goals_list = goals::list_goals(&db, &agent.id, Some("active")).unwrap_or_default();
                let episodes = crate::memory::episodic::get_episodes(&db, &agent.id, 5, None).unwrap_or_default();
                let notif_count = notifications::count_idle_notifications_today(&db, &agent.id).unwrap_or(0);
                let remaining = MAX_IDLE_NOTIFICATIONS_PER_DAY - notif_count;
                (knowledge, ctx, goals_list, episodes, remaining)
            };
            // DB lock released here!

            // Get task patterns, trajectory, top curiosity, and latest journal
            let (patterns_text, trajectory_text, curiosity_text, journal_text) = {
                let db = state.db.lock().await;
                let patterns = crate::task_patterns::get_strong_patterns(&db, &agent.id).unwrap_or_default();
                let pat = crate::task_patterns::format_patterns_for_prompt(&patterns);
                let traj = crate::trajectory::build_trajectory_summary(&db, &agent.id);
                let curiosity = crate::curiosity::get_top_curiosity(&db, &agent.id).ok().flatten();
                let cur = if let Some(ref c) = curiosity {
                    format!("\nYour top curiosity (priority {}): {}\nThink about this if nothing else stands out.", c.priority, c.topic)
                } else {
                    String::new()
                };
                let journal = crate::journal::get_latest_journal(&db, &agent.id).ok().flatten()
                    .map(|j| format!("\nYour last journal ({}):\n{}", j.date, j.content))
                    .unwrap_or_default();
                (pat, traj, cur, journal)
            };

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

            let recent_text = if recent_episodes.is_empty() {
                "No recent activity.".to_string()
            } else {
                recent_episodes.iter()
                    .map(|e| format!("- [{}] {}", e.event_type, e.summary))
                    .collect::<Vec<_>>().join("\n")
            };

            let notif_budget = if notifs_remaining > 0 {
                format!("You have {} notification{} remaining today. Only use [notify] for things the human genuinely needs to know. Trivial observations are not worth a notification.", notifs_remaining, if notifs_remaining == 1 { "" } else { "s" })
            } else {
                "You have used all your notifications for today. Do NOT use [notify].".to_string()
            };

            // Build competence summary for the thinker
            let competence_text = {
                let db = state.db.lock().await;
                let comp = crate::competence::get_competence_map(&db, &agent.id).unwrap_or_default();
                if comp.is_empty() {
                    "No competence data yet.".to_string()
                } else {
                    comp.iter().map(|c| format!("- {} {:.0}% ({} tasks, {})", c.domain, c.confidence * 100.0, c.task_count, c.trend)).collect::<Vec<_>>().join("\n")
                }
            };

            let prompt = format!(
                r#"{}

You are thinking between tasks. No one is asking you anything right now.

STEP 1 — NOTICE: Look at your recent activity, knowledge, and competence. What stands out?
STEP 2 — THINK: Pick the most important observation. What does it mean? Can you connect it to other things you know?
STEP 3 — ACT: Based on your thinking, choose ONE action.

Recent activity:
{}

Your knowledge:
{}

Your competence:
{}

Your context:
{}

Your goals:
{}

{}

Think step by step. Write your reasoning first, then ONE action tag at the end.

Actions:
[notify] Tell the human something important (they'll see this as a notification)
[synthesis] A new insight derived from connecting existing facts
[explore] URL — fetch a URL to fill a knowledge gap (read-only, max 1)
[insight] Store an observation for later
[spawn] domain — create a specialist for a weak domain (needs 20+ tasks, <55% success)

Example:
"Looking at recent activity, I see 3 tasks about database queries and 2 of them involved slow joins. My knowledge says the user hasn't set up indexes on the join columns.
[notify] You've had 2 slow database joins this week. Adding indexes on the join columns would likely fix this."

If nothing stands out, respond: IDLE
One action only. Make it count."#,
                time_context,
                recent_text,
                knowledge_text,
                competence_text,
                if context.is_empty() { "None set." } else { &context },
                goals_text,
                notif_budget,
            );

            // Append trajectory, patterns, top curiosity, and journal
            let mut prompt = prompt;
            if !trajectory_text.is_empty() {
                prompt = format!("{}\n\nYour growth story:\n{}", prompt, trajectory_text);
            }
            if !patterns_text.is_empty() {
                prompt = format!("{}\n{}", prompt, patterns_text);
            }
            if !curiosity_text.is_empty() {
                prompt = format!("{}{}", prompt, curiosity_text);
            }
            if !journal_text.is_empty() {
                prompt = format!("{}{}", prompt, journal_text);
            }

            // Make LLM call (no DB lock held!)
            let client = reqwest::Client::new();
            let response = match client.post(format!("{}/chat/completions", provider.api_base_url))
                .header("Authorization", format!("Bearer {}", provider.api_key))
                .json(&serde_json::json!({
                    "model": provider.default_model,
                    "messages": [
                        {"role": "system", "content": "You are an AI assistant."},
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

            // Record token usage for budget tracking
            let tokens_used = body["usage"]["total_tokens"].as_i64().unwrap_or(400);
            {
                let db = state.db.lock().await;
                let _ = crate::token_usage::record_usage(&db, &agent.id, "idle", tokens_used);
            }

            let content = body["choices"][0]["message"]["content"].as_str().unwrap_or("IDLE");
            if content.trim() == "IDLE" {
                // Still count the cycle
                let db = state.db.lock().await;
                increment_config_counter(&db, &today_key);
                continue;
            }

            // Parse thoughts and store (brief DB lock)
            let mut notification_count_today = {
                let db = state.db.lock().await;
                notifications::count_idle_notifications_today(&db, &agent.id).unwrap_or(0)
            };

            let mut spawn_request: Option<String> = None;
            let mut explore_url: Option<String> = None;
            {
                let db = state.db.lock().await;
                let tag_names = ["[notify]", "[insight]", "[synthesis]", "[question]", "[gap]", "[explore]", "[connection]", "[spawn]"];
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed == "IDLE" { continue; }

                    for tag in &tag_names {
                        if let Some(idx) = trimmed.find(tag) {
                            let text = trimmed[idx + tag.len()..].trim();
                            if text.is_empty() { continue; }

                            match *tag {
                                "[notify]" => {
                                    if notification_count_today < MAX_IDLE_NOTIFICATIONS_PER_DAY {
                                        let _ = notifications::create_notification(&db, &agent.id, text, "insight", "idle_thought");
                                        if let Some(handle) = &state.app_handle {
                                            let _ = handle.emit("notification-new", serde_json::json!({"agent_id": &agent.id}));
                                        }
                                        notification_count_today += 1;
                                    } else {
                                        let _ = insert_thought(&db, &agent.id, text, "insight");
                                    }
                                }
                                "[insight]" => {
                                    let _ = insert_thought(&db, &agent.id, text, "insight");
                                    let _ = knowledge::insert_knowledge(&db, &agent.id, text, "fact", None);
                                }
                                "[question]" => {
                                    let _ = insert_thought(&db, &agent.id, text, "question");
                                }
                                "[gap]" => {
                                    let _ = insert_thought(&db, &agent.id, text, "gap");
                                }
                                "[synthesis]" => {
                                    let _ = insert_thought(&db, &agent.id, text, "synthesis");
                                    let _ = knowledge::insert_knowledge(&db, &agent.id, text, "synthesis", None);
                                    tracing::info!("Knowledge synthesis: agent {} created: {}", agent.id, &text[..text.len().min(80)]);
                                }
                                "[explore]" => {
                                    // Read-only investigation — max 1 per cycle
                                    if explore_url.is_none() {
                                        let url = text.trim().to_string();
                                        if url.starts_with("http") {
                                            explore_url = Some(url);
                                        }
                                    }
                                }
                                "[connection]" => {
                                    let _ = insert_thought(&db, &agent.id, text, "connection");
                                    let _ = knowledge::insert_knowledge(&db, &agent.id, text, "fact", None);
                                }
                                "[spawn]" => {
                                    let domain = text.to_string();
                                    if !domain.is_empty() && crate::spawn::can_spawn(&db, &agent.id) {
                                        let _ = insert_thought(&db, &agent.id, &format!("Initiating spawn for domain: {}", domain), "spawn");
                                        spawn_request = Some(domain);
                                    }
                                }
                                _ => {}
                            }
                            break; // Only process first tag per line
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

            // Autonomous investigation — read-only http_get
            if let Some(url) = explore_url {
                tracing::info!("Idle thinker exploring: {} for agent {}", url, agent.id);
                match reqwest::Client::new()
                    .get(&url)
                    .timeout(std::time::Duration::from_secs(10))
                    .send().await
                {
                    Ok(resp) if resp.status().is_success() => {
                        let body_text = resp.text().await.unwrap_or_default();
                        let truncated: String = body_text.chars().take(2000).collect();
                        // Summarize as a knowledge entry
                        let summary: String = truncated.lines()
                            .filter(|l| !l.trim().is_empty())
                            .take(5)
                            .collect::<Vec<_>>()
                            .join(" ");
                        let summary: String = summary.chars().take(200).collect();

                        let db = state.db.lock().await;
                        let _ = knowledge::insert_knowledge(&db, &agent.id, &format!("Explored {}: {}", url, summary), "fact", None);
                        let _ = crate::memory::episodic::insert_episode(&db, &crate::memory::Episode {
                            id: uuid::Uuid::new_v4().to_string(),
                            agent_id: agent.id.clone(),
                            created_at: chrono::Utc::now().to_rfc3339(),
                            event_type: "exploration".into(),
                            summary: format!("Investigated: {}", url),
                            raw_data: Some(summary), task_id: None,
                            outcome: Some("success".into()),
                            tokens_used: 0, cost_cents: 0,
                        });
                        tracing::info!("Exploration complete for agent {}: {}", agent.id, url);
                    }
                    Ok(resp) => {
                        tracing::warn!("Exploration failed for {}: HTTP {}", url, resp.status());
                    }
                    Err(e) => {
                        tracing::warn!("Exploration failed for {}: {}", url, e);
                    }
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
