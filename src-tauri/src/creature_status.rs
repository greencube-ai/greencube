use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatureStatus {
    pub mood: String,
    pub top_strength: Option<(String, f64)>,
    pub top_weakness: Option<(String, f64)>,
    pub knowledge_count: i64,
    pub last_reflection_summary: Option<String>,
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
    }
}
