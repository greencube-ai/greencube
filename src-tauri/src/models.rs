use serde::Serialize;
use std::path::PathBuf;

pub struct ModelEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub filename: &'static str,
    pub repo: &'static str,
    pub size_bytes: u64,
}

pub static MODELS: &[ModelEntry] = &[
    ModelEntry {
        id: "phi4-mini",
        name: "Phi-4 Mini Instruct (2.5 GB)",
        filename: "Phi-4-mini-instruct-Q4_K_M.gguf",
        repo: "lmstudio-community/Phi-4-mini-instruct-GGUF",
        size_bytes: 2_490_000_000,
    },
    ModelEntry {
        id: "llama3-8b",
        name: "Llama 3.3 8B Instruct (4.9 GB)",
        filename: "Meta-Llama-3.3-8B-Instruct-Q4_K_M.gguf",
        repo: "lmstudio-community/Meta-Llama-3.3-8B-Instruct-GGUF",
        size_bytes: 4_920_000_000,
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

#[derive(Serialize, Clone)]
pub struct ModelStatus {
    pub id: String,
    pub name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub downloaded: bool,
    pub path: String,
}

pub fn list_model_statuses() -> Vec<ModelStatus> {
    MODELS
        .iter()
        .map(|m| {
            let path = models_dir().join(m.filename);
            let downloaded = path.exists();
            ModelStatus {
                id: m.id.to_string(),
                name: m.name.to_string(),
                filename: m.filename.to_string(),
                size_bytes: m.size_bytes,
                downloaded,
                path: path.to_string_lossy().to_string(),
            }
        })
        .collect()
}
