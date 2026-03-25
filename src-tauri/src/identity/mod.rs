pub mod registry;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // private_key stored for future message signing (v0.2 multiplayer)
pub struct Agent {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    pub system_prompt: String,
    #[serde(skip_serializing)]
    pub public_key: Vec<u8>,
    #[serde(skip)]
    pub private_key: Vec<u8>,
    pub tools_allowed: Vec<String>,
    pub max_spend_cents: i64,
    pub total_tasks: i64,
    pub successful_tasks: i64,
    pub total_spend_cents: i64,
}

/// Serializable agent for API responses (no private key, includes computed fields)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    pub system_prompt: String,
    pub tools_allowed: Vec<String>,
    pub max_spend_cents: i64,
    pub total_tasks: i64,
    pub successful_tasks: i64,
    pub total_spend_cents: i64,
    pub reputation: f64,
    pub public_key: String, // base64-encoded
}

impl Agent {
    pub fn reputation(&self) -> f64 {
        if self.total_tasks == 0 {
            0.5 // Starting reputation
        } else {
            (self.successful_tasks as f64 / self.total_tasks as f64) * 0.8 + 0.5 * 0.2
        }
    }

    pub fn to_response(&self) -> AgentResponse {
        use base64::Engine;
        AgentResponse {
            id: self.id.clone(),
            name: self.name.clone(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            status: self.status.clone(),
            system_prompt: self.system_prompt.clone(),
            tools_allowed: self.tools_allowed.clone(),
            max_spend_cents: self.max_spend_cents,
            total_tasks: self.total_tasks,
            successful_tasks: self.successful_tasks,
            total_spend_cents: self.total_spend_cents,
            reputation: self.reputation(),
            public_key: base64::engine::general_purpose::STANDARD.encode(&self.public_key),
        }
    }
}
