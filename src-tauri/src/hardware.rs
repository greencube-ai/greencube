use std::path::PathBuf;

use sysinfo::System;

use crate::models::{self, ModelEntry};

/// Everything we know about the hardware, plus which model was selected.
#[derive(Debug, Clone, Copy)]
pub struct HardwareInfo {
    pub total_ram_gb: u64,
    pub selected_model: &'static ModelEntry,
}

impl HardwareInfo {
    /// Full path to the selected model's canonical GGUF file.
    /// C:\models is the agreed location for now - Phase 5 will make this configurable.
    pub fn model_path(&self) -> PathBuf {
        models::models_dir().join(self.selected_model.filename)
    }
}

pub fn select_model_for_ram(total_ram_gb: u64) -> &'static ModelEntry {
    models::recommended_model(total_ram_gb)
}

/// Read system RAM and decide which model to use.
///
/// Selection rules (RAM-only for now - GPU support comes later):
///   < 16 GB  -> Llama 3.3 8B
///   16-31 GB -> Qwen3 14B
///   32-39 GB -> Gemma 4 26B-A4B MoE
///   >= 40 GB -> Gemma 4 31B
pub fn detect() -> HardwareInfo {
    let mut sys = System::new();
    sys.refresh_memory();

    // sysinfo returns bytes - convert to GB
    let total_ram_gb = sys.total_memory() / (1024 * 1024 * 1024);
    let selected_model = select_model_for_ram(total_ram_gb);

    HardwareInfo {
        total_ram_gb,
        selected_model,
    }
}

/// Check which model files are actually present on disk, and return the best
/// available one. Always falls back to a smaller model - never a bigger one -
/// so we do not load a model the machine cannot handle.
///
/// Returns (full_path, display_name), or None if no model files are found.
pub fn find_available_model() -> Option<(String, String)> {
    let hw = detect();

    for model in models::fallback_models(hw.total_ram_gb) {
        if let Some(path) = models::installed_path(model) {
            return Some((
                path.to_string_lossy().to_string(),
                model.display_name.to_string(),
            ));
        }
    }

    None
}

/// Returns the best available fast model and, if a reasoning pair exists on disk,
/// also the reasoning model.  Both are returned as (path, display_name).
///
/// For 32+ GB machines the pairing is always: Gemma 4 26B MoE (fast) + Gemma 4 31B (reasoning).
/// Both models can run on any 32+ GB machine; the MoE handles quick queries while the 31B
/// handles tasks that need deeper reasoning.  Only one is held in memory at a time.
pub fn find_model_pair() -> (Option<(String, String)>, Option<(String, String)>) {
    let hw = detect();
    let ram = hw.total_ram_gb;

    // For 32+ GB: always use 26B MoE as fast and 31B as reasoning (when available on disk).
    if ram >= 32 {
        let moe = models::MODELS.iter().find(|m| m.id == "gemma4-26b-moe");
        let dense = models::MODELS.iter().find(|m| m.id == "gemma4-31b");

        let moe_path = moe.and_then(|m| models::installed_path(m));
        let dense_path = dense.and_then(|m| models::installed_path(m));

        // If the MoE is on disk, use it as the fast model.
        if let (Some(m), Some(path)) = (moe, moe_path) {
            let fast = Some((path.to_string_lossy().to_string(), m.display_name.to_string()));
            let reasoning = if let (Some(d), Some(dpath)) = (dense, dense_path) {
                Some((dpath.to_string_lossy().to_string(), d.display_name.to_string()))
            } else {
                None
            };
            return (fast, reasoning);
        }

        // MoE not on disk yet — fall back to just the 31B with no reasoning upgrade.
        if let (Some(d), Some(dpath)) = (dense, dense_path) {
            return (Some((dpath.to_string_lossy().to_string(), d.display_name.to_string())), None);
        }
    }

    // For < 32 GB: use the best available model and its declared reasoning pair.
    let fast_entry = models::fallback_models(ram)
        .into_iter()
        .find(|m| models::installed_path(m).is_some());

    let fast = fast_entry.and_then(|m| {
        models::installed_path(m).map(|p| (p.to_string_lossy().to_string(), m.display_name.to_string()))
    });

    let reasoning = fast_entry
        .and_then(models::find_reasoning_pair)
        .and_then(|rm| {
            models::installed_path(rm)
                .map(|p| (p.to_string_lossy().to_string(), rm.display_name.to_string()))
        });

    (fast, reasoning)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_hardware() {
        let info = detect();

        println!("\n--- Hardware Detection ---");
        println!("Total RAM : {} GB", info.total_ram_gb);
        println!("Model     : {}", info.selected_model.display_name);
        println!("File      : {}", info.selected_model.filename);
        println!("Path      : {}", info.model_path().display());
        println!("--------------------------");

        // RAM should be a sane value (more than 0, less than 10TB)
        assert!(info.total_ram_gb > 0, "RAM should be detectable");
        assert!(info.total_ram_gb < 10_000, "RAM value looks wrong");
    }

    #[test]
    fn test_select_model_for_ram_uses_expected_tiers() {
        assert_eq!(select_model_for_ram(8).id, "llama3-8b");
        assert_eq!(select_model_for_ram(13).id, "llama3-8b");
        assert_eq!(select_model_for_ram(14).id, "llama3-8b");
        assert_eq!(select_model_for_ram(15).id, "llama3-8b");
        assert_eq!(select_model_for_ram(16).id, "qwen3-14b");
        assert_eq!(select_model_for_ram(32).id, "gemma4-26b-moe");
        assert_eq!(select_model_for_ram(39).id, "gemma4-26b-moe");
        assert_eq!(select_model_for_ram(40).id, "gemma4-31b");
        assert_eq!(select_model_for_ram(64).id, "gemma4-31b");
    }
}
