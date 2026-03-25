use thiserror::Error;

#[derive(Error, Debug)]
pub enum GreenCubeError {
    // Database errors
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    // Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    // Agent errors
    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Agent error: {0}")]
    AgentError(String),

    #[error("Agent with name '{0}' already exists")]
    DuplicateAgent(String),

    // LLM proxy errors
    #[error("LLM API error: {0}")]
    LlmError(String),

    #[error("LLM API unreachable: {0}")]
    LlmUnreachable(String),

    // Sandbox errors
    #[error("Docker not available")]
    DockerNotAvailable,

    #[error("Sandbox error: {0}")]
    SandboxError(String),

    #[error("Sandbox timeout after {0} seconds")]
    SandboxTimeout(u64),

    // Permission errors
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Spending cap exceeded")]
    SpendingCapExceeded,

    // General
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

// For Tauri commands — must implement Serialize
impl serde::Serialize for GreenCubeError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

// For axum responses
impl axum::response::IntoResponse for GreenCubeError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;

        let (status, message) = match &self {
            GreenCubeError::AgentNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            GreenCubeError::Validation(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            GreenCubeError::DuplicateAgent(_) => (StatusCode::CONFLICT, self.to_string()),
            GreenCubeError::PermissionDenied(_) => (StatusCode::FORBIDDEN, self.to_string()),
            GreenCubeError::SpendingCapExceeded => (StatusCode::FORBIDDEN, self.to_string()),
            GreenCubeError::DockerNotAvailable => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GreenCubeError::LlmUnreachable(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            GreenCubeError::LlmError(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = serde_json::json!({ "error": message });
        (status, axum::Json(body)).into_response()
    }
}
