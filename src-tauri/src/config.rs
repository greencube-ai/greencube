use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub llm: LlmConfig,
    pub server: ServerConfig,
    pub sandbox: SandboxConfig,
    pub ui: UiConfig,
    #[serde(default)]
    pub idle: IdleConfig,
    #[serde(default)]
    pub cost: CostConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub api_base_url: String,
    pub api_key: String,
    pub default_model: String,
    #[serde(default = "default_memory_mode")]
    pub memory_mode: String, // "off", "keyword"
    #[serde(default = "default_true")]
    pub self_reflection_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub image: String,
    pub cpu_limit_cores: f64,
    pub memory_limit_mb: u64,
    pub timeout_seconds: u64,
    pub network_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub onboarding_complete: bool,
    #[serde(default = "default_true")]
    pub alive_mode: bool, // kept for config compat, always true — no modes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostConfig {
    pub daily_background_token_budget: u64,
}

impl Default for CostConfig {
    fn default() -> Self {
        Self { daily_background_token_budget: 50000 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdleConfig {
    pub idle_thinking_enabled: bool,
    pub idle_minutes_before_think: u64,
    pub max_thoughts_per_cycle: u64,
    pub max_daily_idle_cycles: u64,
}

impl Default for IdleConfig {
    fn default() -> Self {
        Self {
            idle_thinking_enabled: true,
            idle_minutes_before_think: 15,
            max_thoughts_per_cycle: 3,
            max_daily_idle_cycles: 10,
        }
    }
}

fn default_memory_mode() -> String { "keyword".into() }
fn default_true() -> bool { true }

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            llm: LlmConfig {
                api_base_url: "https://api.openai.com/v1".into(),
                api_key: String::new(),
                default_model: "gpt-4o".into(),
                memory_mode: "keyword".into(),
                self_reflection_enabled: true,
            },
            server: ServerConfig {
                host: "127.0.0.1".into(),
                port: 9000,
            },
            sandbox: SandboxConfig {
                image: "python:3.12-slim".into(),
                cpu_limit_cores: 1.0,
                memory_limit_mb: 512,
                timeout_seconds: 300,
                network_enabled: false,
            },
            ui: UiConfig {
                onboarding_complete: false,
                alive_mode: true, // Alive Mode by default — the creature is the product
            },
            idle: IdleConfig::default(),
            cost: CostConfig::default(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".greencube")
}

pub fn load_config() -> anyhow::Result<AppConfig> {
    let path = config_dir().join("config.toml");
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        match toml::from_str(&content) {
            Ok(config) => Ok(config),
            Err(e) => {
                // Config corrupted — rename to .bak, create fresh default
                tracing::warn!("Config corrupted: {}. Creating fresh default.", e);
                let bak = config_dir().join("config.toml.bak");
                let _ = std::fs::rename(&path, &bak);
                let config = AppConfig::default();
                save_config(&config)?;
                Ok(config)
            }
        }
    } else {
        let config = AppConfig::default();
        save_config(&config)?;
        Ok(config)
    }
}

pub fn save_config(config: &AppConfig) -> anyhow::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let content = toml::to_string_pretty(config)?;
    std::fs::write(dir.join("config.toml"), content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_serializes() {
        let config = AppConfig::default();
        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        let parsed: AppConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.llm.api_base_url, "https://api.openai.com/v1");
        assert_eq!(parsed.server.port, 9000);
        assert!(!parsed.ui.onboarding_complete);
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("config.toml");

        let config = AppConfig::default();
        let content = toml::to_string_pretty(&config).expect("serialize");
        std::fs::write(&path, &content).expect("write");

        let loaded: AppConfig =
            toml::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");
        assert_eq!(loaded.llm.default_model, "gpt-4o");
        assert_eq!(loaded.sandbox.memory_limit_mb, 512);
    }

    #[test]
    fn test_default_values() {
        let config = AppConfig::default();
        assert_eq!(config.llm.api_key, "");
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.sandbox.image, "python:3.12-slim");
        assert!(!config.sandbox.network_enabled);
    }
}
