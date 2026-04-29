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
///   >= 32 GB -> Mistral Small 3.1 24B
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
        assert_eq!(select_model_for_ram(16).id, "qwen3-14b");
        assert_eq!(select_model_for_ram(48).id, "mistral-small-24b");
    }
}
