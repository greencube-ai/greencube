use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

use crate::identity::registry;
use crate::state::AppState;

/// GET /brain — if one agent, show brain directly. If multiple, show list.
pub async fn brain(State(state): State<Arc<AppState>>) -> Response {
    let agents = {
        let db = state.db.lock().await;
        registry::list_agents(&db).unwrap_or_default()
    };

    if agents.is_empty() {
        return (StatusCode::OK, "no agents yet. connect an agent first.\n").into_response();
    }

    if agents.len() == 1 {
        return render_brain(&state, &agents[0]).await.into_response();
    }

    // Multiple agents: show list
    let mut out = String::from("--- greencube habitat ---\n");
    for (i, agent) in agents.iter().enumerate() {
        let mood = {
            let db = state.db.lock().await;
            crate::mood::get_mood(&db, &agent.id)
        };
        out.push_str(&format!(
            "{}. {} ({} tasks, mood: {})\n",
            i + 1, agent.name, agent.total_tasks, mood
        ));
    }
    out.push_str("\ncurl localhost:9000/brain/1   <- pick a number\n");
    (StatusCode::OK, out).into_response()
}

/// GET /brain/:n
pub async fn brain_by_index(
    State(state): State<Arc<AppState>>,
    Path(index): Path<usize>,
) -> Response {
    let agents = {
        let db = state.db.lock().await;
        registry::list_agents(&db).unwrap_or_default()
    };

    if index == 0 || index > agents.len() {
        return (StatusCode::NOT_FOUND, format!("agent {} not found. you have {} agents.\n", index, agents.len())).into_response();
    }

    render_brain(&state, &agents[index - 1]).await.into_response()
}

/// Phrases that indicate ignorance, not knowledge. Filter these from display.
const JUNK_KNOWLEDGE: &[&str] = &[
    "i don't have any information",
    "i don't know",
    "i'm not sure",
    "i don't have access",
    "i cannot",
    "no context or goals",
    "no information available",
    "there is no context",
    "limits my abi",
    "i do not have",
    "store the observation that there is no",
    "enhancing my built-in",
    "failed in unknown",
    "failed in context",
    "failed in general",
    "unresolved issue",
    "needs investig",
    "warning suggests",
];

fn is_junk_knowledge(content: &str) -> bool {
    let lower = content.to_lowercase();
    JUNK_KNOWLEDGE.iter().any(|p| lower.contains(p))
        || lower.starts_with("failed in")
        || lower.starts_with("correction")
        || lower.starts_with("praised")
        || lower.starts_with("user disapproved")
        || lower.starts_with("user approved")
}

async fn render_brain(state: &AppState, agent: &crate::identity::Agent) -> String {
    let budget = state.config.read().await.cost.daily_background_token_budget;
    let db = state.db.lock().await;

    let mood = crate::mood::get_mood(&db, &agent.id);
    let success_pct = if agent.total_tasks > 0 {
        (agent.successful_tasks as f64 / agent.total_tasks as f64 * 100.0) as i64
    } else { 0 };

    let budget_ok = crate::token_usage::has_budget_remaining(&db, &agent.id, 0, budget).unwrap_or(true);
    let bg_tokens = crate::token_usage::get_background_usage_today(&db, &agent.id).unwrap_or(0);
    let bg_cost = bg_tokens as f64 / 1000.0 * 0.01;

    // Knowledge — filter out junk, failures, corrections, stale
    let knowledge = crate::knowledge::list_knowledge(&db, &agent.id, 50).unwrap_or_default();
    let clean_facts: Vec<_> = knowledge.iter()
        .filter(|k| !k.stale)
        .filter(|k| k.category == "fact" || k.category == "preference" || k.category == "skill" || k.category == "synthesis")
        .filter(|k| !is_junk_knowledge(&k.content))
        .collect();

    let competence = crate::competence::get_competence_map(&db, &agent.id).unwrap_or_default();

    // Recent: only task_start episodes, deduplicated
    let episodes = crate::memory::episodic::get_episodes(&db, &agent.id, 50, None).unwrap_or_default();
    let mut seen_tasks: std::collections::HashSet<String> = std::collections::HashSet::new();
    let task_starts: Vec<_> = episodes.iter()
        .filter(|ep| ep.event_type == "task_start")
        .filter(|ep| {
            if let Some(ref tid) = ep.task_id {
                seen_tasks.insert(tid.clone())
            } else { false }
        })
        .take(5)
        .collect();

    // Match each task_start with its task_end for outcome
    let task_outcomes: Vec<(String, String, String)> = task_starts.iter().map(|ep| {
        let prompt = ep.summary.replace("Task started: ", "");
        let prompt_short: String = prompt.chars().take(50).collect();
        let prompt_display = if prompt.chars().count() > 50 { format!("{}...", prompt_short) } else { prompt_short };

        let outcome = if let Some(ref tid) = ep.task_id {
            episodes.iter()
                .find(|e| e.event_type == "task_end" && e.task_id.as_deref() == Some(tid))
                .and_then(|e| e.outcome.clone())
                .unwrap_or_else(|| "pending".to_string())
        } else { "unknown".to_string() };

        (ep.created_at.clone(), prompt_display, outcome)
    }).collect();

    let mistakes_prevented = get_counter(&db, &agent.id, "mistakes_prevented");
    let facts_used = get_counter(&db, &agent.id, "facts_used");
    let corrections_applied = get_counter(&db, &agent.id, "corrections_applied");

    drop(db);

    let mut out = String::new();

    // Header
    out.push_str(&format!("\u{1F9E0} {} | mood: {} | {} tasks | {}% success\n", agent.name, mood, agent.total_tasks, success_pct));
    if !budget_ok {
        out.push_str("   learning paused (daily budget reached, resumes tomorrow)\n");
    }

    // Learned
    if clean_facts.is_empty() {
        out.push_str("learned: nothing yet\n");
    } else {
        out.push_str("learned:\n");
        for k in clean_facts.iter().take(15) {
            let content: String = k.content.chars().take(80).collect();
            out.push_str(&format!("  \u{2022} {}\n", content));
        }
        if clean_facts.len() > 15 {
            out.push_str(&format!("  ... and {} more\n", clean_facts.len() - 15));
        }
    }

    // Good at
    if !competence.is_empty() {
        out.push_str("good at:\n");
        for c in &competence {
            let pct = (c.confidence * 100.0) as i64;
            let filled = (c.confidence * 5.0) as usize;
            let empty = 5 - filled.min(5);
            let bar = format!("{}{}", "\u{2588}".repeat(filled.min(5)), "\u{2591}".repeat(empty));
            out.push_str(&format!("  \u{2022} {:12} {} {}%\n", c.domain, bar, pct));
        }
    }

    // Improvements
    if mistakes_prevented > 0 || facts_used > 0 || corrections_applied > 0 {
        out.push_str("improvements:\n");
        if mistakes_prevented > 0 { out.push_str(&format!("  \u{2022} {} mistakes prevented\n", mistakes_prevented)); }
        if facts_used > 0 { out.push_str(&format!("  \u{2022} {} facts used in tasks\n", facts_used)); }
        if corrections_applied > 0 { out.push_str(&format!("  \u{2022} {} corrections applied\n", corrections_applied)); }
    }

    // Recent tasks
    if !task_outcomes.is_empty() {
        out.push_str("recent:\n");
        let now = chrono::Utc::now();
        for (ts, prompt, outcome) in &task_outcomes {
            let ago = if let Ok(t) = chrono::DateTime::parse_from_rfc3339(ts) {
                let mins = now.signed_duration_since(t).num_minutes();
                if mins < 1 { "just now".to_string() }
                else if mins < 60 { format!("{}m ago", mins) }
                else if mins < 1440 { format!("{}h ago", mins / 60) }
                else { format!("{}d ago", mins / 1440) }
            } else { "?".to_string() };
            let icon = if outcome == "success" { "\u{2713}" } else if outcome == "failure" { "\u{2717}" } else { "\u{2022}" };
            out.push_str(&format!("  {} {:8} \"{}\"\n", icon, ago, prompt));
        }
    }

    // Cost
    out.push_str(&format!("today: ~{} tokens (~${:.3})\n", bg_tokens, bg_cost));
    out.push('\n');

    out
}

/// GET /status — one line summary with cost
pub async fn status(State(state): State<Arc<AppState>>) -> String {
    let db = state.db.lock().await;
    let agents = registry::list_agents(&db).unwrap_or_default();

    if agents.is_empty() {
        return "running | 0 agents | no activity yet\n".to_string();
    }

    let total_tasks: i64 = agents.iter().map(|a| a.total_tasks).sum();
    let total_knowledge: i64 = agents.iter().map(|a| {
        crate::knowledge::list_knowledge(&db, &a.id, 1000).map(|k| k.len() as i64).unwrap_or(0)
    }).sum();

    let total_bg_tokens: i64 = agents.iter().map(|a| {
        crate::token_usage::get_background_usage_today(&db, &a.id).unwrap_or(0)
    }).sum();
    let bg_cost = total_bg_tokens as f64 / 1000.0 * 0.01;

    if agents.len() == 1 {
        let mood = crate::mood::get_mood(&db, &agents[0].id);
        format!("running | {} tasks | {} facts learned | mood: {} | overhead today: ~${:.3}\n", total_tasks, total_knowledge, mood, bg_cost)
    } else {
        format!("running | {} agents | {} tasks | {} facts learned | overhead today: ~${:.3}\n", agents.len(), total_tasks, total_knowledge, bg_cost)
    }
}

/// GET /log — last 20 activity entries in plain english
pub async fn log(State(state): State<Arc<AppState>>) -> String {
    let db = state.db.lock().await;
    let agents = registry::list_agents(&db).unwrap_or_default();

    if agents.is_empty() {
        return "no activity yet.\n".to_string();
    }

    let entries = crate::permissions::audit::get_recent_activity(&db, 40).unwrap_or_default();

    let agent_map: std::collections::HashMap<String, String> = agents.iter()
        .map(|a| (a.id.clone(), a.name.clone()))
        .collect();

    let mut all_entries: Vec<(String, String, String)> = entries.iter().map(|e| {
        let name = agent_map.get(&e.agent_id).cloned().unwrap_or_else(|| "unknown".into());
        (e.created_at.clone(), name, e.action_detail.clone())
    }).collect();

    all_entries.truncate(20);

    let now = chrono::Utc::now();
    let mut out = String::new();

    for (ts, name, detail) in &all_entries {
        let ago = if let Ok(t) = chrono::DateTime::parse_from_rfc3339(ts) {
            let mins = now.signed_duration_since(t).num_minutes();
            if mins < 1 { "just now".to_string() }
            else if mins < 60 { format!("{}min ago", mins) }
            else if mins < 1440 { format!("{}hr ago", mins / 60) }
            else { format!("{}d ago", mins / 1440) }
        } else {
            "?".to_string()
        };
        let detail_short: String = detail.chars().take(70).collect();
        let prefix = if agents.len() > 1 { format!("[{}] ", name) } else { String::new() };
        out.push_str(&format!("  {:10} {}{}\n", ago, prefix, detail_short));
    }

    if out.is_empty() { "no activity yet.\n".to_string() } else { out }
}

// --- Counter helpers ---

fn get_counter(conn: &rusqlite::Connection, agent_id: &str, name: &str) -> i64 {
    let key = format!("counter_{}_{}", agent_id, name);
    conn.query_row(
        "SELECT CAST(value AS INTEGER) FROM config_store WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get(0),
    ).unwrap_or(0)
}

pub fn increment_counter(conn: &rusqlite::Connection, agent_id: &str, name: &str) {
    let key = format!("counter_{}_{}", agent_id, name);
    let current = get_counter(conn, agent_id, name);
    let _ = conn.execute(
        "INSERT INTO config_store (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
        rusqlite::params![key, (current + 1).to_string()],
    );
}
