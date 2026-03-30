use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

use crate::identity::registry;
use crate::state::AppState;

/// GET /brain — if one agent, show brain directly. If multiple, show list.
/// GET /brain/:n — show brain for agent at position n (1-indexed).
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

async fn render_brain(state: &AppState, agent: &crate::identity::Agent) -> String {
    let db = state.db.lock().await;

    // Mood
    let mood = crate::mood::get_mood(&db, &agent.id);
    let success_pct = if agent.total_tasks > 0 {
        (agent.successful_tasks as f64 / agent.total_tasks as f64 * 100.0) as i64
    } else { 0 };

    // Knowledge
    let knowledge = crate::knowledge::list_knowledge(&db, &agent.id, 50).unwrap_or_default();
    let non_stale: Vec<_> = knowledge.iter().filter(|k| !k.stale).collect();

    // Competence
    let competence = crate::competence::get_competence_map(&db, &agent.id).unwrap_or_default();

    // Recent episodes
    let episodes = crate::memory::episodic::get_episodes(&db, &agent.id, 10, None).unwrap_or_default();

    drop(db);

    let mut out = String::new();

    // Header
    out.push_str(&format!("---\ngreencube agent: {}\n", agent.name));
    out.push_str(&format!("mood: {}\n", mood));
    out.push_str(&format!("tasks: {} | success: {}%\n", agent.total_tasks, success_pct));
    out.push_str("---\n");

    // Knowledge
    if non_stale.is_empty() {
        out.push_str("what i know: nothing yet\n");
    } else {
        out.push_str(&format!("what i know ({} facts):\n", non_stale.len()));
        for k in non_stale.iter().take(15) {
            let content: String = k.content.chars().take(80).collect();
            out.push_str(&format!("  - {}\n", content));
        }
        if non_stale.len() > 15 {
            out.push_str(&format!("  ... and {} more\n", non_stale.len() - 15));
        }
    }
    out.push_str("---\n");

    // Competence bars
    if competence.is_empty() {
        out.push_str("what im good at: no data yet\n");
    } else {
        out.push_str("what im good at:\n");
        for c in &competence {
            let pct = (c.confidence * 100.0) as i64;
            let filled = (c.confidence * 10.0) as usize;
            let empty = 10 - filled.min(10);
            let bar: String = "\u{2588}".repeat(filled) + &"\u{2591}".repeat(empty);
            out.push_str(&format!("  {:12} {} {}%\n", c.domain, bar, pct));
        }
    }
    out.push_str("---\n");

    // Recent activity
    if episodes.is_empty() {
        out.push_str("recent: no activity yet\n");
    } else {
        out.push_str("recent:\n");
        let now = chrono::Utc::now();
        for ep in episodes.iter().take(10) {
            let ago = if let Ok(t) = chrono::DateTime::parse_from_rfc3339(&ep.created_at) {
                let mins = now.signed_duration_since(t).num_minutes();
                if mins < 1 { "just now".to_string() }
                else if mins < 60 { format!("{}min ago", mins) }
                else if mins < 1440 { format!("{}hr ago", mins / 60) }
                else { format!("{}d ago", mins / 1440) }
            } else {
                "?".to_string()
            };
            let summary: String = ep.summary.chars().take(60).collect();
            out.push_str(&format!("  {:10} {}\n", ago, summary));
        }
    }
    out.push('\n');

    out
}

/// GET /status — one line summary
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

    if agents.len() == 1 {
        let mood = crate::mood::get_mood(&db, &agents[0].id);
        format!("running | {} tasks | {} facts learned | mood: {}\n", total_tasks, total_knowledge, mood)
    } else {
        format!("running | {} agents | {} tasks | {} facts learned\n", agents.len(), total_tasks, total_knowledge)
    }
}

/// GET /log — last 20 activity entries in plain english
pub async fn log(State(state): State<Arc<AppState>>) -> String {
    let db = state.db.lock().await;
    let agents = registry::list_agents(&db).unwrap_or_default();

    if agents.is_empty() {
        return "no activity yet.\n".to_string();
    }

    // Collect recent audit entries across all agents
    let entries = crate::permissions::audit::get_recent_activity(&db, 40).unwrap_or_default();

    // Map agent_id to name for display
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

    if out.is_empty() {
        "no activity yet.\n".to_string()
    } else {
        out
    }
}
