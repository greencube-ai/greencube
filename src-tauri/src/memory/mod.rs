pub mod episodic;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub agent_id: String,
    pub created_at: String,
    pub event_type: String,
    pub summary: String,
    pub raw_data: Option<String>,
    pub task_id: Option<String>,
    pub outcome: Option<String>,
    pub tokens_used: i64,
    pub cost_cents: i64,
}
