use std::sync::{Arc, Mutex};

use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{params::LlamaModelParams, LlamaModel},
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::{hardware, models};

// --- App state (lives for the entire duration the app is running) ---

/// Holds the loaded model in memory so we don't reload it on every message.
pub(crate) struct LoadedModel {
    backend: LlamaBackend,
    model: LlamaModel,
}

// llama.cpp's internal structures use raw C pointers, which Rust marks as
// non-thread-safe by default. We wrap them in a Mutex so only one thread
// accesses them at a time, making it safe to share across threads.
unsafe impl Send for LoadedModel {}
unsafe impl Sync for LoadedModel {}

/// Wrap a user message in the ChatML format that Qwen3 (and Llama 3) expect.
/// Without this the model doesn't know where its turn ends, so it keeps
/// generating — inventing more "user" messages and answering them itself.
fn apply_chat_template(user_message: &str) -> String {
    format!(
        "<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n<|im_start|>user\n{user_message}<|im_end|>\n<|im_start|>assistant\n"
    )
}

/// Load a model from disk, trying GPU acceleration first and falling back to
/// CPU if Vulkan is unavailable or the GPU runs out of memory.
fn load_model_with_fallback(path: &str) -> Result<LoadedModel, String> {
    let backend =
        LlamaBackend::init().map_err(|e| format!("Failed to initialize llama.cpp: {e}"))?;

    // Try GPU: offload all layers to Vulkan (999 is "as many as possible").
    let gpu_params = LlamaModelParams::default().with_n_gpu_layers(999);
    match LlamaModel::load_from_file(&backend, path, &gpu_params) {
        Ok(model) => {
            log::info!("Model loaded with GPU acceleration (Vulkan)");
            return Ok(LoadedModel { backend, model });
        }
        Err(e) => {
            log::warn!("GPU loading failed ({e}), retrying on CPU");
        }
    }

    // CPU fallback — works on any hardware.
    let cpu_params = LlamaModelParams::default();
    let model = LlamaModel::load_from_file(&backend, path, &cpu_params)
        .map_err(|e| format!("Failed to load model: {e}"))?;
    log::info!("Model loaded on CPU (no GPU acceleration)");
    Ok(LoadedModel { backend, model })
}

/// Tauri managed state — shared across all command calls.
pub struct AppState {
    /// Display name of the model in use (e.g. "Qwen3 14B").
    pub model_name: String,
    /// Full path to the GGUF file on disk.
    pub model_path: String,
    /// The loaded model, initialized on first message to keep startup fast.
    pub loaded: Arc<Mutex<Option<LoadedModel>>>,
}

// --- Commands ---

/// What the frontend receives when it asks about the current model.
#[derive(Serialize)]
pub struct ModelInfo {
    pub model_name: String,
    pub model_path: String,
}

/// Returns which model is currently selected and where it lives on disk.
/// The frontend can use this to display "Running Qwen3 14B" in the UI.
#[tauri::command]
pub fn get_model_info(state: State<AppState>) -> ModelInfo {
    ModelInfo {
        model_name: state.model_name.clone(),
        model_path: state.model_path.clone(),
    }
}

/// Send a message to the AI and get a response.
///
/// The model is loaded from disk on the first call, then kept in memory
/// for all subsequent calls. This makes the first message slow (~10-30s)
/// but every message after that fast (~1-5s).
///
/// Streaming (word-by-word output) comes in Phase 4.
#[tauri::command]
pub fn send_message(prompt: String, state: State<AppState>) -> Result<String, String> {
    if state.model_path.is_empty() {
        return Err("No model file found in C:\\models. Download a model first.".to_string());
    }

    let mut guard = state
        .loaded
        .lock()
        .map_err(|e| format!("Failed to acquire model lock: {e}"))?;

    if guard.is_none() {
        log::info!("Loading model: {}", state.model_path);
        *guard = Some(load_model_with_fallback(&state.model_path)?);
    }

    let loaded = guard.as_ref().unwrap();

    crate::inference::generate_with(&loaded.backend, &loaded.model, &apply_chat_template(&prompt), 1024)
        .map_err(|e| format!("Inference failed: {e}"))
}

/// Streaming version of send_message.
///
/// Instead of returning the full response at the end, this command emits
/// one Tauri event per token as the model generates them:
///
///   "chat-token"  — payload: String (one text fragment, e.g. " Paris")
///   "chat-done"   — payload: null (signals the response is complete)
///   "chat-error"  — payload: String (error message, if something went wrong)
///
/// The frontend should listen for these events before calling this command.
#[tauri::command]
pub fn send_message_streaming(
    prompt: String,
    state: State<AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if state.model_path.is_empty() {
        app.emit("chat-error", "No model file found in C:\\models").ok();
        return Err("No model available".to_string());
    }

    let model_path = state.model_path.clone();
    let loaded = state.loaded.clone();

    // Spawn on a background thread so the UI stays responsive while the
    // model loads and generates. The command returns immediately.
    std::thread::spawn(move || {
        let mut guard = match loaded.lock() {
            Ok(g) => g,
            Err(e) => {
                app.emit("chat-error", format!("Lock error: {e}")).ok();
                return;
            }
        };

        if guard.is_none() {
            log::info!("Loading model: {model_path}");
            match load_model_with_fallback(&model_path) {
                Ok(m) => *guard = Some(m),
                Err(e) => {
                    app.emit("chat-error", &e).ok();
                    return;
                }
            }
        }

        let loaded = guard.as_ref().unwrap();
        let result = crate::inference::generate_streaming(
            &loaded.backend,
            &loaded.model,
            &apply_chat_template(&prompt),
            1024,
            |token| { app.emit("chat-token", token).ok(); },
        );

        match result {
            Ok(()) => { app.emit("chat-done", ()).ok(); }
            Err(e) => { app.emit("chat-error", e.to_string()).ok(); }
        }
    });

    Ok(())
}

// --- Setup / first-run ---

/// Returns true if no model has been downloaded yet.
/// The frontend shows the setup/download screen when this is true.
#[tauri::command]
pub fn is_first_run() -> bool {
    models::list_model_statuses().iter().all(|m| !m.downloaded)
}

// --- Model management ---

#[derive(Serialize, Clone)]
struct DownloadProgress {
    model_id: String,
    downloaded_bytes: u64,
    total_bytes: u64,
}

#[derive(Serialize, Clone)]
struct DownloadComplete {
    model_id: String,
    path: String,
}

#[derive(Serialize, Clone)]
struct DownloadError {
    model_id: String,
    error: String,
}

/// Returns every known model and whether it is already downloaded.
#[tauri::command]
pub fn list_models() -> Vec<models::ModelStatus> {
    models::list_model_statuses_for_ram(hardware::detect().total_ram_gb)
}

/// Download a model from HuggingFace in the background.
///
/// Events emitted during download:
///   "download-progress"  — { model_id, downloaded_bytes, total_bytes }
///   "download-complete"  — { model_id, path }
///   "download-error"     — { model_id, error }
///
/// `hf_token` is optional; pass it only for gated/private models.
#[tauri::command]
pub fn download_model(
    model_id: String,
    hf_token: Option<String>,
    app: AppHandle,
) -> Result<(), String> {
    let model = models::MODELS
        .iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("Unknown model id: {model_id}"))?;

    let url = models::download_url(model);
    let dest_path =
        models::installed_path(model).unwrap_or_else(|| models::models_dir().join(model.filename));
    let size_hint = model.size_bytes;
    let mid = model_id.clone();

    std::thread::spawn(move || {
        if let Err(e) = run_download(&app, &mid, &url, &dest_path, size_hint, hf_token.as_deref()) {
            app.emit(
                "download-error",
                DownloadError {
                    model_id: mid,
                    error: e.to_string(),
                },
            )
            .ok();
        }
    });

    Ok(())
}

fn run_download(
    app: &AppHandle,
    model_id: &str,
    url: &str,
    dest: &std::path::Path,
    size_hint: u64,
    hf_token: Option<&str>,
) -> anyhow::Result<()> {
    use std::io::{Read, Write};

    if dest.exists() {
        app.emit(
            "download-complete",
            DownloadComplete {
                model_id: model_id.to_string(),
                path: dest.to_string_lossy().to_string(),
            },
        )
        .ok();
        return Ok(());
    }

    let client = reqwest::blocking::Client::builder().timeout(None).build()?;

    let mut req = client.get(url);
    if let Some(token) = hf_token {
        req = req.header("Authorization", format!("Bearer {token}"));
    }

    let mut response = req
        .send()
        .map_err(|e| anyhow::anyhow!("Request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("Server returned {}", response.status()));
    }

    let total = response.content_length().unwrap_or(size_hint);

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = std::fs::File::create(dest)?;
    let mut downloaded: u64 = 0;
    let mut buf = vec![0u8; 65536];

    loop {
        let n = response.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;
        app.emit(
            "download-progress",
            DownloadProgress {
                model_id: model_id.to_string(),
                downloaded_bytes: downloaded,
                total_bytes: total,
            },
        )
        .ok();
    }

    app.emit(
        "download-complete",
        DownloadComplete {
            model_id: model_id.to_string(),
            path: dest.to_string_lossy().to_string(),
        },
    )
    .ok();

    Ok(())
}
