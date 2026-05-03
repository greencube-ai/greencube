use std::sync::{Arc, Mutex};

use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{params::LlamaModelParams, LlamaModel},
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::{hardware, models};

const SYSTEM_PROMPT: &str =
    "You are GreenCube, a helpful coding assistant. You write clean, working code. \
     When asked to build something, you provide complete implementations.";

#[derive(Debug, Clone, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

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

/// Format a full conversation history into the prompt string the model expects.
/// The system prompt is prepended server-side; the frontend only sends
/// user/assistant turns. The trailing assistant header is left open so the
/// model continues from there.
fn apply_chat_template(model_name: &str, messages: &[ChatMessage]) -> String {
    if model_name.contains("Llama") {
        apply_llama3_template(messages)
    } else {
        apply_chatml_template(messages)
    }
}

fn apply_llama3_template(messages: &[ChatMessage]) -> String {
    let mut out = String::from("<|begin_of_text|>");
    out.push_str(&format!(
        "<|start_header_id|>system<|end_header_id|>\n\n{SYSTEM_PROMPT}<|eot_id|>"
    ));
    for msg in messages {
        let role = match msg.role.as_str() {
            "assistant" => "assistant",
            _ => "user",
        };
        out.push_str(&format!(
            "<|start_header_id|>{role}<|end_header_id|>\n\n{}<|eot_id|>",
            msg.content
        ));
    }
    out.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
    out
}

/// ChatML format used by Qwen3 and Qwen3.5.
fn apply_chatml_template(messages: &[ChatMessage]) -> String {
    let mut out = format!("<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n");
    for msg in messages {
        let role = match msg.role.as_str() {
            "assistant" => "assistant",
            _ => "user",
        };
        out.push_str(&format!(
            "<|im_start|>{role}\n{}<|im_end|>\n",
            msg.content
        ));
    }
    out.push_str("<|im_start|>assistant\n");
    out
}

/// Load a model from disk, trying GPU acceleration first and falling back to
/// CPU if Vulkan is unavailable or the GPU runs out of memory.
fn load_model_with_fallback(path: &str) -> Result<LoadedModel, String> {
    let backend =
        LlamaBackend::init().map_err(|e| format!("Failed to initialize llama.cpp: {e}"))?;

    const GPU_LAYERS_REQUESTED: u32 = 999;
    let gpu_params = LlamaModelParams::default().with_n_gpu_layers(GPU_LAYERS_REQUESTED);
    match LlamaModel::load_from_file(&backend, path, &gpu_params) {
        Ok(model) => {
            log::info!(
                "Backend: Vulkan GPU. Requested up to {} offload layers (llama.cpp will print actual layer count above).",
                GPU_LAYERS_REQUESTED
            );
            return Ok(LoadedModel { backend, model });
        }
        Err(e) => {
            log::warn!("GPU loading failed ({e}), retrying on CPU");
        }
    }

    let cpu_params = LlamaModelParams::default();
    let model = LlamaModel::load_from_file(&backend, path, &cpu_params)
        .map_err(|e| format!("Failed to load model: {e}"))?;
    log::info!("Backend: CPU only (Vulkan unavailable). Generation will be much slower.");
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

/// Send a full conversation to the AI and get a response.
///
/// The model is loaded from disk on the first call, then kept in memory
/// for all subsequent calls. This makes the first message slow (~10-30s)
/// but every message after that fast (~1-5s).
///
/// `messages` is the entire chat history in order; the last entry should be
/// the new user message. The system prompt is added server-side.
#[tauri::command]
pub fn send_message(
    messages: Vec<ChatMessage>,
    state: State<AppState>,
) -> Result<String, String> {
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
    let n_ctx = ctx_size_for(&state.model_path);

    crate::inference::generate_with(
        &loaded.backend,
        &loaded.model,
        &apply_chat_template(&state.model_name, &messages),
        1024,
        n_ctx,
    )
    .map_err(|e| format!("Inference failed: {e}"))
}

/// Decide n_ctx based on the loaded model file and current RAM.
/// Falls back to 4096 if we can't identify the model.
fn ctx_size_for(model_path: &str) -> u32 {
    let filename = std::path::Path::new(model_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let total_ram_gb = hardware::detect().total_ram_gb;
    models::lookup_by_filename(filename)
        .map(|m| models::recommended_ctx(m, total_ram_gb))
        .unwrap_or(4096)
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
    messages: Vec<ChatMessage>,
    state: State<AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if state.model_path.is_empty() {
        app.emit("chat-error", "No model file found in C:\\models").ok();
        return Err("No model available".to_string());
    }

    let model_path = state.model_path.clone();
    let model_name = state.model_name.clone();
    let loaded = state.loaded.clone();
    let n_ctx = ctx_size_for(&model_path);
    let prompt = apply_chat_template(&model_name, &messages);

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
            &prompt,
            1024,
            n_ctx,
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
