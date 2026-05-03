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
}

/// Ordered from lightest to heaviest so RAM-based selection stays data-driven.
pub static MODELS: &[ModelEntry] = &[
    ModelEntry {
        id: "llama3-8b",
        name: "Llama 3.1 8B Instruct (4.9 GB)",
        display_name: "Llama 3.1 8B",
        filename: "Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
        legacy_filenames: &[],
        repo: "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF",
        size_bytes: 4_920_000_000,
        min_ram_gb: 0,
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
    },
    ModelEntry {
        id: "mistral-small-24b",
        name: "Mistral Small 3.1 24B Instruct (14.0 GB)",
        display_name: "Mistral Small 3.1 24B",
        filename: "mistral-small-3.1-24b-instruct-2503-q4_k_m.gguf",
        legacy_filenames: &["Mistral-Small-3.1-24B-Instruct-2503-Q4_K_M.gguf"],
        repo: "openfree/Mistral-Small-3.1-24B-Instruct-2503-Q4_K_M-GGUF",
        size_bytes: 14_000_000_000,
        min_ram_gb: 32,
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
    let selected_index = MODELS
        .iter()
        .position(|model| model.id == selected.id)
        .unwrap_or(0);

    MODELS[..=selected_index].iter().rev().collect()
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
    fn test_list_model_statuses_for_ram_marks_qwen_as_recommended() {
        let statuses = list_model_statuses_for_ram(16);

        assert_eq!(
            statuses.first().map(|status| status.id.as_str()),
            Some("qwen3.5-9b")
        );
        assert!(statuses
            .iter()
            .any(|status| status.id == "qwen3.5-9b" && status.recommended));
        assert_eq!(
            statuses.iter().filter(|status| status.recommended).count(),
            1
        );
    }
}
