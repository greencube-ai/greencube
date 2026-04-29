use std::path::PathBuf;

use sysinfo::System;

/// Which model the app will use, based on detected hardware.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectedModel {
    Phi4Mini,
    MistralSmall,
    Llama3_8B,
}

impl SelectedModel {
    /// The filename of the GGUF file for this model.
    pub fn filename(&self) -> &str {
        match self {
            SelectedModel::Phi4Mini => "Phi-4-mini-instruct-Q4_K_M.gguf",
            SelectedModel::MistralSmall => "Mistral-Small-3.1-24B-Instruct-2503-Q4_K_M.gguf",
            SelectedModel::Llama3_8B => "Meta-Llama-3.3-8B-Instruct-Q4_K_M.gguf",
        }
    }

    /// Human-readable name shown in the UI.
    pub fn display_name(&self) -> &str {
        match self {
            SelectedModel::Phi4Mini => "Phi-4 Mini",
            SelectedModel::MistralSmall => "Mistral Small 3.x",
            SelectedModel::Llama3_8B => "Llama 3.3 8B",
        }
    }
}

/// Everything we know about the hardware, plus which model was selected.
#[derive(Debug, Clone)]
pub struct HardwareInfo {
    pub total_ram_gb: u64,
    pub selected_model: SelectedModel,
}

impl HardwareInfo {
    /// Full path to the selected model's GGUF file.
    /// C:\models is the agreed location for now — Phase 5 will make this configurable.
    pub fn model_path(&self) -> PathBuf {
        PathBuf::from("C:\\models").join(self.selected_model.filename())
    }
}

/// Read system RAM and decide which model to use.
///
/// Selection rules (RAM-only for now — GPU support comes later):
///   < 8 GB  → Phi-4 Mini      (lightest, runs on anything)
///   8–16 GB → Mistral Small   (balanced)
///   > 16 GB → Llama 3.3 8B   (best quality)
pub fn detect() -> HardwareInfo {
    let mut sys = System::new();
    sys.refresh_memory();

    // sysinfo returns bytes — convert to GB
    let total_ram_gb = sys.total_memory() / (1024 * 1024 * 1024);

    let selected_model = if total_ram_gb < 8 {
        SelectedModel::Phi4Mini
    } else if total_ram_gb <= 16 {
        SelectedModel::MistralSmall
    } else {
        SelectedModel::Llama3_8B
    };

    HardwareInfo {
        total_ram_gb,
        selected_model,
    }
}

/// Check which model files are actually present on disk, and return the best
/// available one. Always falls back to a smaller model — never a bigger one —
/// so we don't load a model the machine can't handle.
///
/// Returns (full_path, display_name), or None if no model files are found.
pub fn find_available_model() -> Option<(String, String)> {
    let hw = detect();

    // Build a candidate list starting from the hardware-selected model,
    // falling back to progressively smaller ones.
    let candidates: Vec<SelectedModel> = match hw.selected_model {
        SelectedModel::Llama3_8B => vec![
            SelectedModel::Llama3_8B,
            SelectedModel::MistralSmall,
            SelectedModel::Phi4Mini,
        ],
        SelectedModel::MistralSmall => vec![
            SelectedModel::MistralSmall,
            SelectedModel::Phi4Mini,
        ],
        SelectedModel::Phi4Mini => vec![
            SelectedModel::Phi4Mini,
        ],
    };

    for model in candidates {
        let path = PathBuf::from("C:\\models").join(model.filename());
        if path.exists() {
            return Some((
                path.to_string_lossy().to_string(),
                model.display_name().to_string(),
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
        println!("Model     : {}", info.selected_model.display_name());
        println!("File      : {}", info.selected_model.filename());
        println!("Path      : {}", info.model_path().display());
        println!("--------------------------");

        // RAM should be a sane value (more than 0, less than 10TB)
        assert!(info.total_ram_gb > 0, "RAM should be detectable");
        assert!(info.total_ram_gb < 10_000, "RAM value looks wrong");
    }
}
