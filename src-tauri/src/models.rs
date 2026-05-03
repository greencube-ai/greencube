use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub display_name: &'static str,
    pub filename: &'static str,
    pub legacy_filenames: &'static [&'static str],
    pub repo: &'static str,
    pub size_bytes: u64,
    pub min_ram_gb: u64,
    /// ID of a heavier model to use for complex/reasoning queries. None if this is already the best.
    pub reasoning_pair_id: Option<&'static str>,
}

/// Ordered from lightest to heaviest so RAM-based selection stays data-driven.
pub static MODELS: &[ModelEntry] = &[
    ModelEntry {
        id: "phi4-mini",
        name: "Phi-4 Mini Instruct (2.4 GB)",
        display_name: "Phi-4 Mini",
        filename: "Phi-4-mini-instruct-Q4_K_M.gguf",
        legacy_filenames: &[],
        repo: "bartowski/Phi-4-mini-instruct-GGUF",
        size_bytes: 2_500_000_000,
        min_ram_gb: 0,
        reasoning_pair_id: None,
    },
    // Legacy fallback: kept so users who already downloaded Llama 3.1 8B
    // before the Phi-4 Mini default still get auto-detected. Not the recommended pick.
    ModelEntry {
        id: "llama3-8b",
        name: "Llama 3.1 8B Instruct (4.9 GB)",
        display_name: "Llama 3.1 8B",
        filename: "Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
        legacy_filenames: &[],
        repo: "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF",
        size_bytes: 4_920_000_000,
        min_ram_gb: 0,
        reasoning_pair_id: None,
    },
    ModelEntry {
        id: "qwen3.5-9b",
        name: "Qwen3.5 9B Instruct (5.7 GB)",
        display_name: "Qwen3.5 9B",
        filename: "Qwen3.5-9B-Q4_K_M.gguf",
        legacy_filenames: &["Qwen_Qwen3.5-9B-Q4_K_M.gguf"],
        repo: "unsloth/Qwen3.5-9B-GGUF",
        size_bytes: 5_680_522_464,
        min_ram_gb: 0,
        reasoning_pair_id: None,
    },
    ModelEntry {
        id: "qwen3-14b",
        name: "Qwen3 14B Instruct (9.0 GB)",
        display_name: "Qwen3 14B",
        filename: "Qwen3-14B-Q4_K_M.gguf",
        legacy_filenames: &[],
        repo: "Qwen/Qwen3-14B-GGUF",
        size_bytes: 9_000_000_000,
        min_ram_gb: 20,
        reasoning_pair_id: None,
    },
    ModelEntry {
        id: "gemma4-26b-moe",
        name: "Gemma 4 26B-A4B MoE Instruct (17.0 GB)",
        display_name: "Gemma 4 26B MoE",
        filename: "google_gemma-4-26B-A4B-it-Q4_K_M.gguf",
        legacy_filenames: &[],
        repo: "bartowski/google_gemma-4-26B-A4B-it-GGUF",
        size_bytes: 17_040_000_000,
        min_ram_gb: 32,
        reasoning_pair_id: Some("gemma4-31b"),
    },
    ModelEntry {
        id: "gemma4-31b",
        name: "Gemma 4 31B Instruct (18.3 GB)",
        display_name: "Gemma 4 31B",
        filename: "gemma-4-31B-it-Q4_K_M.gguf",
        legacy_filenames: &[],
        repo: "unsloth/gemma-4-31B-it-GGUF",
        size_bytes: 18_300_000_000,
        min_ram_gb: 40,
        reasoning_pair_id: None,
    },
];

pub fn models_dir() -> PathBuf {
    PathBuf::from("C:\\models")
}

pub fn download_url(model: &ModelEntry) -> String {
    format!(
        "https://huggingface.co/{}/resolve/main/{}",
        model.repo, model.filename
    )
}

/// Pick a context window size that fits in available RAM.
/// Larger ctx = more KV-cache memory; on tight machines we shrink it for big models.
pub fn recommended_ctx(model: &ModelEntry, total_ram_gb: u64) -> u32 {
    if model.size_bytes > 8_000_000_000 && total_ram_gb < 20 {
        4096
    } else {
        8192
    }
}

pub fn lookup_by_filename(filename: &str) -> Option<&'static ModelEntry> {
    MODELS.iter().find(|m| {
        m.filename == filename || m.legacy_filenames.iter().any(|legacy| *legacy == filename)
    })
}

pub fn recommended_model(total_ram_gb: u64) -> &'static ModelEntry {
    let mut selected = &MODELS[0];

    for model in MODELS {
        if total_ram_gb >= model.min_ram_gb {
            selected = model;
        } else {
            break;
        }
    }

    selected
}

pub fn fallback_models(total_ram_gb: u64) -> Vec<&'static ModelEntry> {
    let selected = recommended_model(total_ram_gb);
    fallback_models_from(selected)
}

pub fn fallback_models_from(selected: &'static ModelEntry) -> Vec<&'static ModelEntry> {
    let selected_index = MODELS
        .iter()
        .position(|model| model.id == selected.id)
        .unwrap_or(0);

    MODELS[..=selected_index].iter().rev().collect()
}

/// Returns the heavier reasoning-optimised partner for `model`, if one is defined.
pub fn find_reasoning_pair(model: &'static ModelEntry) -> Option<&'static ModelEntry> {
    model
        .reasoning_pair_id
        .and_then(|id| MODELS.iter().find(|m| m.id == id))
}

pub fn installed_path(model: &ModelEntry) -> Option<PathBuf> {
    for filename in std::iter::once(model.filename).chain(model.legacy_filenames.iter().copied()) {
        let path = models_dir().join(filename);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

#[derive(Serialize, Clone)]
pub struct ModelStatus {
    pub id: String,
    pub name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub min_ram_gb: u64,
    pub recommended: bool,
    pub downloaded: bool,
    pub path: String,
}

pub fn list_model_statuses() -> Vec<ModelStatus> {
    build_model_statuses(None)
}

pub fn list_model_statuses_for_ram(total_ram_gb: u64) -> Vec<ModelStatus> {
    build_model_statuses(Some(recommended_model(total_ram_gb).id))
}

pub fn list_model_statuses_for_recommended(recommended_id: &str) -> Vec<ModelStatus> {
    build_model_statuses(Some(recommended_id))
}

fn build_model_statuses(recommended_id: Option<&str>) -> Vec<ModelStatus> {
    let mut statuses: Vec<ModelStatus> = MODELS
        .iter()
        .map(|m| {
            let installed_path = installed_path(m);
            let downloaded = installed_path.is_some();
            let path = installed_path.unwrap_or_else(|| models_dir().join(m.filename));

            ModelStatus {
                id: m.id.to_string(),
                name: m.name.to_string(),
                filename: m.filename.to_string(),
                size_bytes: m.size_bytes,
                min_ram_gb: m.min_ram_gb,
                recommended: recommended_id == Some(m.id),
                downloaded,
                path: path.to_string_lossy().to_string(),
            }
        })
        .collect();

    if let Some(recommended_id) = recommended_id {
        statuses.sort_by_key(|status| if status.id == recommended_id { 0 } else { 1 });
    }

    statuses
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_model_statuses_for_ram_marks_recommended() {
        // 16 GB: qwen3-14b is too big (min 20), qwen3.5-9b is the heaviest <=16
        let statuses_16 = list_model_statuses_for_ram(16);
        assert!(statuses_16
            .iter()
            .any(|s| s.id == "qwen3.5-9b" && s.recommended));

        // 24 GB: qwen3-14b kicks in (min 20)
        let statuses_24 = list_model_statuses_for_ram(24);
        assert!(statuses_24
            .iter()
            .any(|s| s.id == "qwen3-14b" && s.recommended));

        // 32 GB: gemma 26B MoE is the standard (paired with 31B for reasoning)
        let statuses_32 = list_model_statuses_for_ram(32);
        assert!(statuses_32
            .iter()
            .any(|s| s.id == "gemma4-26b-moe" && s.recommended));

        // 40 GB: gemma 31B becomes the default
        let statuses_40 = list_model_statuses_for_ram(40);
        assert!(statuses_40
            .iter()
            .any(|s| s.id == "gemma4-31b" && s.recommended));
    }
}
