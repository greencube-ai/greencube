use std::sync::{Arc, Mutex};

use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{params::LlamaModelParams, LlamaModel},
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::{db, hardware, models};

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

/// Build the full ChatML prompt from conversation history + new user message.
/// Prior turns are included so the model has full context of the conversation.
fn apply_chat_template(history: &[db::StoredMessage], new_user_message: &str) -> String {
    let mut prompt =
        String::from("<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n");
    for msg in history {
        let role = match msg.role.as_str() {
            "assistant" => "assistant",
            _ => "user",
        };
        prompt.push_str(&format!(
            "<|im_start|>{role}\n{}<|im_end|>\n",
            msg.content
        ));
    }
    prompt.push_str(&format!(
        "<|im_start|>user\n{new_user_message}<|im_end|>\n<|im_start|>assistant\n"
    ));
    prompt
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
    /// SQLite conversation history database.
    pub db: Arc<Mutex<db::Db>>,
}

// --- Commands ---

/// What the frontend receives when it asks about the current model.
#[derive(Serialize)]
pub struct ModelInfo {
    pub model_name: String,
    pub model_path: String,
}

/// Returns which model is currently selected and where it lives on disk.
#[tauri::command]
pub fn get_model_info(state: State<AppState>) -> ModelInfo {
    ModelInfo {
        model_name: state.model_name.clone(),
        model_path: state.model_path.clone(),
    }
}

/// Streaming version of send_message.
///
/// Accepts an optional `conversation_id`. When omitted a new conversation is
/// created and its id is returned so the frontend can track it.
///
/// Events emitted during generation:
///   "chat-token"  — payload: String  (one text fragment)
///   "chat-done"   — payload: null    (generation complete)
///   "chat-error"  — payload: String  (error message)
///
/// Returns the conversation id (new or existing) synchronously before
/// the background thread starts, so the frontend can update its state
/// immediately without waiting for the first token.
#[tauri::command]
pub fn send_message_streaming(
    prompt: String,
    conversation_id: Option<String>,
    state: State<AppState>,
    app: AppHandle,
) -> Result<String, String> {
    if state.model_path.is_empty() {
        app.emit("chat-error", "No model file found in C:\\models").ok();
        return Err("No model available".to_string());
    }

    // Resolve or create the conversation synchronously (very fast).
    let conv_id = {
        let db = state
            .db
            .lock()
            .map_err(|e| format!("DB lock error: {e}"))?;
        match conversation_id {
            Some(id) => id,
            None => {
                let title: String = prompt.chars().take(60).collect();
                let title = if prompt.chars().count() > 60 {
                    format!("{title}...")
                } else {
                    title
                };
                db.create_conversation(&title)
                    .map_err(|e| format!("Failed to create conversation: {e}"))?
            }
        }
    };

    let model_path = state.model_path.clone();
    let loaded = state.loaded.clone();
    let db_arc = state.db.clone();
    let conv_id_thread = conv_id.clone();

    std::thread::spawn(move || {
        // Load existing history before saving the new message so we don't
        // include it twice when building the prompt.
        let history = db_arc
            .lock()
            .ok()
            .and_then(|db| db.load_messages(&conv_id_thread).ok())
            .unwrap_or_default();

        // Persist the new user message.
        if let Ok(db) = db_arc.lock() {
            db.add_message(&conv_id_thread, "user", &prompt).ok();
        }

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

        let loaded_model = guard.as_ref().unwrap();
        let full_prompt = apply_chat_template(&history, &prompt);

        let mut full_response = String::new();
        let result = crate::inference::generate_streaming(
            &loaded_model.backend,
            &loaded_model.model,
            &full_prompt,
            1024,
            |token| {
                full_response.push_str(&token);
                app.emit("chat-token", token).ok();
            },
        );

        match result {
            Ok(()) => {
                if let Ok(db) = db_arc.lock() {
                    db.add_message(&conv_id_thread, "assistant", &full_response)
                        .ok();
                }
                app.emit("chat-done", ()).ok();
            }
            Err(e) => {
                app.emit("chat-error", e.to_string()).ok();
            }
        }
    });

    Ok(conv_id)
}

// --- Conversation history commands ---

/// Returns all conversations ordered by most recently updated.
#[tauri::command]
pub fn list_conversations(state: State<AppState>) -> Result<Vec<db::ConversationSummary>, String> {
    state
        .db
        .lock()
        .map_err(|e| format!("DB lock error: {e}"))?
        .list_conversations()
        .map_err(|e| e.to_string())
}

/// Returns all messages for a conversation in chronological order.
#[tauri::command]
pub fn load_conversation(id: String, state: State<AppState>) -> Result<Vec<db::StoredMessage>, String> {
    state
        .db
        .lock()
        .map_err(|e| format!("DB lock error: {e}"))?
        .load_messages(&id)
        .map_err(|e| e.to_string())
}

/// Deletes a conversation and all its messages.
#[tauri::command]
pub fn delete_conversation(id: String, state: State<AppState>) -> Result<(), String> {
    state
        .db
        .lock()
        .map_err(|e| format!("DB lock error: {e}"))?
        .delete_conversation(&id)
        .map_err(|e| e.to_string())
}

// --- Setup / first-run ---

/// Returns true if no model has been downloaded yet.
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
        if let Err(e) = run_download(&app, &mid, &url, &dest_path, size_hint, hf_token.as_deref())
        {
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
