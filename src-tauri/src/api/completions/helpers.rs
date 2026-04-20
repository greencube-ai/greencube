use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use tauri::Emitter;

use crate::state::AppState;

/// Helper to emit refresh signals to the frontend
pub(super) fn emit_refresh(state: &AppState) {
    if let Some(handle) = &state.app_handle {
        let _ = handle.emit("activity-refresh", ());
    }
}

pub(super) fn emit_status(state: &AppState, agent_id: &str, status: &str) {
    if let Some(handle) = &state.app_handle {
        let _ = handle.emit(
            "agent-status-change",
            serde_json::json!({"id": agent_id, "status": status}),
        );
    }
}

pub(super) fn error_response(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}

/// SECURITY: Redact sensitive patterns from strings before logging to audit/episodes.
/// Catches Bearer tokens, API keys, passwords, and Authorization headers.
pub(super) fn redact_secrets(s: &str) -> String {
    use std::sync::LazyLock;
    static RE: LazyLock<Vec<(regex::Regex, &'static str)>> = LazyLock::new(|| vec![
        (regex::Regex::new(r"(?i)(bearer\s+)[a-zA-Z0-9_\-\.]+").unwrap(), "${1}[REDACTED]"),
        (regex::Regex::new(r"(?i)(api[_-]?key[=:\s]+)[a-zA-Z0-9_\-\.]+").unwrap(), "${1}[REDACTED]"),
        (regex::Regex::new(r"(?i)(authorization[=:\s]+)[^\s,\}]+").unwrap(), "${1}[REDACTED]"),
        (regex::Regex::new(r"sk-[a-zA-Z0-9]{20,}").unwrap(), "[REDACTED_KEY]"),
    ]);
    let mut result = s.to_string();
    for (re, replacement) in RE.iter() {
        result = re.replace_all(&result, *replacement).to_string();
    }
    result
}

#[cfg(test)]
mod security_tests {
    use super::redact_secrets;

    #[test]
    fn test_redact_bearer_token() {
        let input = r#"curl -H "Authorization: Bearer sk-abc123def456""#;
        let result = redact_secrets(input);
        assert!(!result.contains("sk-abc123def456"), "Bearer token not redacted: {}", result);
        assert!(result.contains("[REDACTED"), "Missing redaction marker: {}", result);
    }

    #[test]
    fn test_redact_api_key_pattern() {
        let input = "api_key=sk-proj-abcdefghijklmnopqrstuvwxyz";
        let result = redact_secrets(input);
        assert!(!result.contains("abcdefghijklmnopqrstuvwxyz"), "API key not redacted: {}", result);
    }

    #[test]
    fn test_redact_sk_prefix() {
        let input = "Using key sk-1234567890abcdefghij1234567890 for requests";
        let result = redact_secrets(input);
        assert!(result.contains("[REDACTED_KEY]"), "sk- pattern not redacted: {}", result);
    }

    #[test]
    fn test_no_false_positive_on_normal_text() {
        let input = "This is a normal shell command: ls -la /tmp";
        let result = redact_secrets(input);
        assert_eq!(result, input, "Normal text was modified");
    }
}
