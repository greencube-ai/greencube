pub mod docker;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i64,
    pub duration_ms: u64,
    pub timed_out: bool,
}

#[derive(Debug, Clone)]
pub struct SandboxOptions {
    pub image: String,
    pub cpu_limit_cores: f64,
    pub memory_limit_mb: u64,
    pub timeout_seconds: u64,
    pub network_enabled: bool,
}
