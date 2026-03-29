use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatureStatus {
    pub mood: String,
    pub top_strength: Option<(String, f64)>,
    pub top_weakness: Option<(String, f64)>,
    pub knowledge_count: i64,
    pub last_reflection_summary: Option<String>,
    pub active_domain: Option<String>,
    pub recent_insight: Option<String>,
    pub pending_investigation: Option<String>,
}

pub fn get_creature_status(conn: &Connection, agent_id: &str) -> CreatureStatus {
    // Knowledge count
    let knowledge_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM knowledge WHERE agent_id = ?1",
        rusqlite::params![agent_id],
        |row| row.get(0),
    ).unwrap_or(0);

    // Competence: best and worst domains
    let competence = crate::competence::get_competence_map(conn, agent_id).unwrap_or_default();
    let top_strength = competence.iter()
        .filter(|c| c.task_count >= 3)
        .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap_or(std::cmp::Ordering::Equal))
        .map(|c| (c.domain.clone(), c.confidence));
    let top_weakness = competence.iter()
        .filter(|c| c.task_count >= 3)
        .min_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap_or(std::cmp::Ordering::Equal))
        .map(|c| (c.domain.clone(), c.confidence));

    // Last reflection summary from episodes
    let last_reflection: Option<String> = conn.query_row(
        "SELECT summary FROM episodes WHERE agent_id = ?1 AND event_type = 'reflection' ORDER BY created_at DESC LIMIT 1",
        rusqlite::params![agent_id],
        |row| row.get(0),
    ).ok();

    // Active domain: most recent task_chain episode's domain
    let active_domain: Option<String> = conn.query_row(
        "SELECT summary FROM episodes WHERE agent_id = ?1 AND event_type = 'task_chain' ORDER BY created_at DESC LIMIT 1",
        rusqlite::params![agent_id],
        |row| row.get::<_, String>(0),
    ).ok().and_then(|s| {
        // Parse "Task in python: success → ..." to extract domain
        s.strip_prefix("Task in ").and_then(|rest| rest.split(':').next()).map(|d| d.to_string())
    });

    // Recent insight: last idle thought of type insight or synthesis
    let recent_insight: Option<String> = conn.query_row(
        "SELECT content FROM idle_thoughts WHERE agent_id = ?1 AND thought_type IN ('insight', 'synthesis', 'connection') ORDER BY created_at DESC LIMIT 1",
        rusqlite::params![agent_id],
        |row| row.get(0),
    ).ok();

    // Pending investigation: top curiosity from the queue
    let pending_investigation = crate::curiosity::get_top_curiosity(conn, agent_id)
        .ok()
        .flatten()
        .map(|c| c.topic);

    // Determine mood
    let mood = if knowledge_count == 0 {
        "waiting".to_string()
    } else if top_weakness.as_ref().map_or(false, |(_, c)| *c < 0.4) {
        "struggling".to_string()
    } else if top_strength.as_ref().map_or(false, |(_, c)| *c > 0.85) {
        "thriving".to_string()
    } else if knowledge_count > 10 {
        "growing".to_string()
    } else {
        "learning".to_string()
    };

    CreatureStatus {
        mood,
        top_strength,
        top_weakness,
        knowledge_count,
        last_reflection_summary: last_reflection,
        active_domain,
        recent_insight,
        pending_investigation,
    }
}
