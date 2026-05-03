use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::Duration;

use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{params::LlamaModelParams, LlamaModel},
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::{db, hardware, models};

// --- App state (lives for the entire duration the app is running) ---

/// Holds the loaded model in memory so we don't reload it on every message.
struct LoadedModel {
    backend: LlamaBackend,
    model: LlamaModel,
}

// llama.cpp's internal structures use raw C pointers, which Rust marks as
// non-thread-safe by default. We wrap them in a Mutex so only one thread
// accesses them at a time, making it safe to share across threads.
unsafe impl Send for LoadedModel {}
unsafe impl Sync for LoadedModel {}

pub(crate) struct LoadedState {
    /// Full path to the GGUF file currently in memory.
    path: String,
    inner: LoadedModel,
}

const MAX_MEMORY_CHARS: usize = 800;
const SUMMARY_MAX_CHARS: usize = 2_400;
const SUMMARY_MESSAGE_MAX_CHARS: usize = 1_200;
const RECENT_HISTORY_MESSAGES: usize = 10;
const SUMMARY_REFRESH_STEP: usize = 4;
const SUMMARY_START_DELAY_MS: u64 = 750;
const SUMMARY_MAX_TOKENS: u32 = 320;

/// Build the full ChatML prompt from memories, conversation history, and the new message.
/// Memories are injected into the system block so the model always has that context.
#[allow(dead_code)]
fn build_system_block(memories: &[db::Memory]) -> String {
    const MAX_MEM_CHARS: usize = 800;
    let mut system = String::from(
        "You are a helpful assistant. \
         When thinking before a response, be concise — keep your reasoning under 200 words.",
    );
    if !memories.is_empty() {
        system.push_str("\n\nThings you know about the user:\n");
        for mem in memories {
            if mem.content.len() <= MAX_MEM_CHARS {
                system.push_str(&format!("- {}\n", mem.content));
            } else {
                let head = mem.content.chars().take(MAX_MEM_CHARS).collect::<String>();
                system.push_str(&format!("- {}… [truncated]\n", head));
            }
        }
    }
    system
}

#[allow(dead_code)]
fn apply_chat_template(
    history: &[db::StoredMessage],
    new_user_message: &str,
    memories: &[db::Memory],
    model_path: &str,
) -> String {
    let system = build_system_block(memories);
    let lower = model_path.to_lowercase();

    if lower.contains("gemma") {
        // Gemma 4 native format — triggers thinking mode correctly.
        let mut prompt = format!("<start_of_turn>system\n{system}<end_of_turn>\n");
        for msg in history {
            let role = if msg.role == "assistant" {
                "model"
            } else {
                "user"
            };
            prompt.push_str(&format!(
                "<start_of_turn>{role}\n{}<end_of_turn>\n",
                msg.content
            ));
        }
        prompt.push_str(&format!(
            "<start_of_turn>user\n{new_user_message}<end_of_turn>\n<start_of_turn>model\n"
        ));
        prompt
    } else {
        // ChatML — used by Qwen3, Phi-4, Llama, and most others.
        let mut prompt = format!("<|im_start|>system\n{system}<|im_end|>\n");
        for msg in history {
            let role = if msg.role == "assistant" {
                "assistant"
            } else {
                "user"
            };
            prompt.push_str(&format!("<|im_start|>{role}\n{}<|im_end|>\n", msg.content));
        }
        prompt.push_str(&format!(
            "<|im_start|>user\n{new_user_message}<|im_end|>\n<|im_start|>assistant\n"
        ));
        prompt
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let mut chars = trimmed.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}... [truncated]")
    } else {
        head
    }
}

fn build_chat_system_block(
    memories: &[db::Memory],
    summary: Option<&db::ConversationContextSummary>,
) -> String {
    let mut system = String::from(
        "You are a helpful assistant. \
         When thinking before a response, be concise - keep your reasoning under 200 words.",
    );

    if let Some(summary) = summary {
        let context = truncate_chars(&summary.summary, SUMMARY_MAX_CHARS);
        if !context.is_empty() {
            system.push_str(
                "\n\nConversation summary of older context. Treat it as background context; \
                 newer raw messages follow separately:\n",
            );
            system.push_str(&context);
        }
    }

    if !memories.is_empty() {
        system.push_str("\n\nThings you know about the user:\n");
        for mem in memories {
            let content = truncate_chars(&mem.content, MAX_MEMORY_CHARS);
            system.push_str(&format!("- {content}\n"));
        }
    }

    system
}

fn apply_model_template(
    system: &str,
    history: &[db::StoredMessage],
    new_user_message: &str,
    model_path: &str,
) -> String {
    let lower = model_path.to_lowercase();

    if lower.contains("gemma") {
        let mut prompt = format!("<start_of_turn>system\n{system}<end_of_turn>\n");
        for msg in history {
            let role = if msg.role == "assistant" {
                "model"
            } else {
                "user"
            };
            prompt.push_str(&format!(
                "<start_of_turn>{role}\n{}<end_of_turn>\n",
                msg.content
            ));
        }
        prompt.push_str(&format!(
            "<start_of_turn>user\n{new_user_message}<end_of_turn>\n<start_of_turn>model\n"
        ));
        prompt
    } else {
        let mut prompt = format!("<|im_start|>system\n{system}<|im_end|>\n");
        for msg in history {
            let role = if msg.role == "assistant" {
                "assistant"
            } else {
                "user"
            };
            prompt.push_str(&format!("<|im_start|>{role}\n{}<|im_end|>\n", msg.content));
        }
        prompt.push_str(&format!(
            "<|im_start|>user\n{new_user_message}<|im_end|>\n<|im_start|>assistant\n"
        ));
        prompt
    }
}

fn apply_chat_template_with_summary(
    summary: Option<&db::ConversationContextSummary>,
    history: &[db::StoredMessage],
    new_user_message: &str,
    memories: &[db::Memory],
    model_path: &str,
) -> String {
    let system = build_chat_system_block(memories, summary);
    apply_model_template(&system, history, new_user_message, model_path)
}

fn format_messages_for_summary(messages: &[db::StoredMessage]) -> String {
    let mut out = String::new();
    for msg in messages {
        let role = if msg.role == "assistant" {
            "Assistant"
        } else {
            "User"
        };
        let content = truncate_chars(&msg.content, SUMMARY_MESSAGE_MAX_CHARS);
        out.push_str(role);
        out.push_str(":\n");
        out.push_str(&content);
        out.push_str("\n\n");
    }
    out.trim().to_string()
}

fn sanitize_summary_output(text: &str) -> String {
    const MARKERS: &[&str] = &[
        "<|im_end|>",
        "<|im_start|>",
        "<end_of_turn>",
        "<|end_of_turn|>",
        "<|eot_id|>",
        "<|end|>",
        "<|endoftext|>",
        "<thought>",
        "</thought>",
        "<channel|>",
        "</channel|>",
        "<channel>",
        "</channel>",
    ];

    let mut cleaned = text.to_string();
    for marker in MARKERS {
        cleaned = cleaned.replace(marker, " ");
    }

    let mut normalized = Vec::new();
    let mut previous_blank = false;
    for raw_line in cleaned.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            if !previous_blank {
                normalized.push(String::new());
            }
            previous_blank = true;
            continue;
        }
        if line.starts_with("```") {
            continue;
        }
        normalized.push(line.to_string());
        previous_blank = false;
    }

    truncate_chars(&normalized.join("\n"), SUMMARY_MAX_CHARS)
}

fn build_summary_prompt(
    existing_summary: Option<&db::ConversationContextSummary>,
    new_messages: &[db::StoredMessage],
    model_path: &str,
) -> String {
    let system = "You maintain a compact running summary of a conversation so another model can \
                  pick up the context quickly. Keep only durable context: goals, constraints, \
                  preferences, decisions, facts, promised follow-up, and open tasks. Do not \
                  include chain-of-thought. Output plain text only.";

    let existing = existing_summary
        .map(|summary| truncate_chars(&summary.summary, SUMMARY_MAX_CHARS))
        .filter(|summary| !summary.is_empty())
        .unwrap_or_else(|| "None yet.".to_string());
    let transcript = format_messages_for_summary(new_messages);
    let user_message = format!(
        "Update the running conversation summary.\n\n\
         Existing summary:\n\
         {existing}\n\n\
         New conversation messages to fold in:\n\
         {transcript}\n\n\
         Return only the updated summary using these exact sections:\n\
         Goals:\n\
         - ...\n\
         Constraints and preferences:\n\
         - ...\n\
         Open tasks:\n\
         - ...\n\
         Key context:\n\
         - ...\n\
         Replace the placeholder dots with real content. Use '- none' when a section is empty."
    );

    apply_model_template(system, &[], &user_message, model_path)
}

fn spawn_summary_refresh(
    loaded: Arc<Mutex<Option<LoadedState>>>,
    db_arc: Arc<Mutex<db::Db>>,
    conversation_id: String,
) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(SUMMARY_START_DELAY_MS));

        let (existing_summary, messages_to_summarize, summarized_through_message_id) = {
            let db = match db_arc.lock() {
                Ok(db) => db,
                Err(e) => {
                    log::warn!("Skipping summary refresh because DB lock failed: {e}");
                    return;
                }
            };

            let existing_summary = match db.get_conversation_summary(&conversation_id) {
                Ok(summary) => summary,
                Err(e) => {
                    log::warn!("Failed to load conversation summary: {e}");
                    return;
                }
            };

            let after_id = existing_summary
                .as_ref()
                .map_or(0, |summary| summary.summarized_through_message_id);
            let unsummarized = match db.load_messages_after_id(&conversation_id, after_id) {
                Ok(messages) => messages,
                Err(e) => {
                    log::warn!("Failed to load unsummarized messages: {e}");
                    return;
                }
            };

            if unsummarized.len() <= RECENT_HISTORY_MESSAGES {
                return;
            }

            let summarizable_len = unsummarized.len().saturating_sub(RECENT_HISTORY_MESSAGES);
            if summarizable_len == 0 {
                return;
            }

            if existing_summary.is_some() && summarizable_len < SUMMARY_REFRESH_STEP {
                return;
            }

            let messages_to_summarize = unsummarized[..summarizable_len].to_vec();
            let summarized_through_message_id = match messages_to_summarize.last() {
                Some(message) => message.id,
                None => return,
            };

            (
                existing_summary,
                messages_to_summarize,
                summarized_through_message_id,
            )
        };

        let guard = match loaded.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return,
            Err(TryLockError::Poisoned(_)) => {
                log::warn!("Skipping summary refresh because the model lock is poisoned");
                return;
            }
        };

        let Some(loaded_state) = guard.as_ref() else {
            return;
        };

        let prompt = build_summary_prompt(
            existing_summary.as_ref(),
            &messages_to_summarize,
            &loaded_state.path,
        );

        // catch_unwind prevents a llama.cpp panic from poisoning the mutex.
        // The MutexGuard is dropped at the end of the enclosing scope (not inside
        // the closure), so if catch_unwind catches a panic the guard unwinds cleanly.
        let inference_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::inference::generate_with(
                &loaded_state.inner.backend,
                &loaded_state.inner.model,
                &prompt,
                SUMMARY_MAX_TOKENS,
            )
        }));

        drop(guard);

        let generated = match inference_result {
            Ok(Ok(text)) => text,
            Ok(Err(e)) => {
                log::warn!("Failed to refresh conversation summary: {e}");
                return;
            }
            Err(payload) => {
                log::warn!("Summary inference panicked: {}", panic_message(payload));
                return;
            }
        };

        let summary = sanitize_summary_output(&generated);
        if summary.is_empty() {
            log::warn!("Skipping summary refresh because the model returned an empty summary");
            return;
        }

        let db = match db_arc.lock() {
            Ok(db) => db,
            Err(e) => {
                log::warn!("Failed to save conversation summary because DB lock failed: {e}");
                return;
            }
        };

        if let Err(e) = db.upsert_conversation_summary(
            &conversation_id,
            &summary,
            summarized_through_message_id,
        ) {
            log::warn!("Failed to store conversation summary: {e}");
        }
    });
}

fn earliest_stop_pos(buffer: &str, stop_strings: &[&str]) -> Option<usize> {
    stop_strings
        .iter()
        .filter_map(|stop| buffer.find(stop))
        .min()
}

fn partial_stop_suffix_start(buffer: &str, stop_strings: &[&str]) -> Option<usize> {
    let max_stop_len = stop_strings.iter().map(|s| s.len()).max().unwrap_or(0);
    let mut boundaries: Vec<usize> = buffer.char_indices().map(|(idx, _)| idx).collect();
    boundaries.push(buffer.len());

    for start in boundaries.into_iter().rev().skip(1) {
        let suffix = &buffer[start..];
        if suffix.len() > max_stop_len {
            break;
        }
        if stop_strings.iter().any(|stop| stop.starts_with(suffix)) {
            return Some(start);
        }
    }

    None
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        (*msg).to_string()
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        msg.clone()
    } else {
        "unknown panic".to_string()
    }
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
    /// Display name of the fast model (e.g. "Gemma 4 26B MoE").
    pub model_name: String,
    /// Full path to the fast GGUF file on disk.
    pub model_path: String,
    /// Display name of the reasoning model, empty if unavailable.
    pub reasoning_model_name: String,
    /// Full path to the reasoning GGUF file, empty if unavailable.
    pub reasoning_model_path: String,
    /// Whichever model is currently loaded; swapped on demand.
    pub loaded: Arc<Mutex<Option<LoadedState>>>,
    /// SQLite conversation history database.
    pub db: Arc<Mutex<db::Db>>,
    /// Developer override: when set, always use this model regardless of auto-selection.
    pub dev_model_override: Arc<Mutex<Option<String>>>, // model id
    /// Set to true to abort the current generation after the next token.
    pub stop_requested: Arc<AtomicBool>,
}

fn current_auto_model_info(state: &AppState) -> (String, String, String, String) {
    let (fast, reasoning) = hardware::find_model_pair();

    let (model_path, model_name) =
        fast.unwrap_or_else(|| (state.model_path.clone(), state.model_name.clone()));
    let (reasoning_model_path, reasoning_model_name) = reasoning.unwrap_or_else(|| {
        (
            state.reasoning_model_path.clone(),
            state.reasoning_model_name.clone(),
        )
    });

    (
        model_path,
        model_name,
        reasoning_model_path,
        reasoning_model_name,
    )
}

pub(crate) fn preload_model_on_startup(
    loaded: Arc<Mutex<Option<LoadedState>>>,
    model_path: String,
    model_name: String,
) {
    if model_path.is_empty() {
        return;
    }

    std::thread::spawn(move || {
        let mut guard = match loaded.lock() {
            Ok(guard) => guard,
            Err(e) => {
                log::warn!("Startup preload lock was poisoned - resetting model state");
                let mut guard = e.into_inner();
                *guard = None;
                guard
            }
        };

        if guard
            .as_ref()
            .is_some_and(|loaded_state| loaded_state.path == model_path)
        {
            return;
        }

        log::info!("Preloading startup model: {} ({})", model_name, model_path);
        match load_model_with_fallback(&model_path) {
            Ok(model) => {
                *guard = Some(LoadedState {
                    path: model_path,
                    inner: model,
                });
                log::info!("Startup model preloaded successfully");
            }
            Err(e) => {
                log::warn!("Startup model preload failed: {e}");
            }
        }
    });
}

// --- Commands ---

/// What the frontend receives when it asks about the current model.
#[derive(Serialize)]
pub struct ModelInfo {
    pub model_name: String,
    pub model_path: String,
    pub reasoning_model_name: String,
    pub reasoning_model_path: String,
}

#[derive(Serialize)]
pub struct HardwareProfile {
    pub total_ram_gb: u64,
    pub cpu_threads: u64,
    pub has_battery: bool,
    pub is_laptop_likely: bool,
    pub on_battery_power: bool,
    pub recommended_model_id: String,
    pub recommended_model_name: String,
    pub recommendation_reason: String,
}

/// Returns which models are available and where they live on disk.
#[tauri::command]
pub fn get_model_info(state: State<AppState>) -> ModelInfo {
    let (model_path, model_name, reasoning_model_path, reasoning_model_name) =
        current_auto_model_info(&state);

    ModelInfo {
        model_name,
        model_path,
        reasoning_model_name,
        reasoning_model_path,
    }
}

#[tauri::command]
pub fn get_hardware_profile() -> HardwareProfile {
    let hw = hardware::detect();
    HardwareProfile {
        total_ram_gb: hw.total_ram_gb,
        cpu_threads: hw.cpu_threads,
        has_battery: hw.has_battery,
        is_laptop_likely: hw.is_laptop_likely,
        on_battery_power: hw.on_battery_power,
        recommended_model_id: hw.selected_model.id.to_string(),
        recommended_model_name: hw.selected_model.display_name.to_string(),
        recommendation_reason: hardware::recommendation_reason(&hw).to_string(),
    }
}

/// Pin a specific model for all responses, bypassing auto-selection. Pass None to clear.
#[tauri::command]
pub fn set_dev_model(model_id: Option<String>, state: State<AppState>) -> Result<(), String> {
    *state
        .dev_model_override
        .lock()
        .map_err(|e| format!("Lock error: {e}"))? = model_id;
    Ok(())
}

/// Returns the currently pinned dev model id, or None if auto-selection is active.
#[tauri::command]
pub fn get_dev_model(state: State<AppState>) -> Option<String> {
    state.dev_model_override.lock().ok()?.clone()
}

/// Asks the current generation to stop after its next token.
#[tauri::command]
pub fn stop_generation(state: State<AppState>) {
    state.stop_requested.store(true, Ordering::Relaxed);
}

/// Returns true if the prompt warrants using the heavier reasoning model.
fn needs_reasoning(prompt: &str) -> bool {
    // Long prompts almost always benefit from a stronger model.
    if prompt.len() > 250 {
        return true;
    }
    let lower = prompt.to_lowercase();
    const SIGNALS: &[&str] = &[
        "explain",
        "why",
        "how does",
        "how do",
        "analyze",
        "analyse",
        "compare",
        "contrast",
        "write",
        "create",
        "implement",
        "code",
        "program",
        "design",
        "build",
        "develop",
        "summarize",
        "summarise",
        "summary",
        "review",
        "evaluate",
        "critique",
        "step by step",
        "step-by-step",
        "difference between",
        "pros and cons",
        "debug",
        "fix this",
        "what's wrong",
        "help me understand",
        "research",
        "essay",
    ];
    SIGNALS.iter().any(|kw| lower.contains(kw))
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
    model_override: Option<bool>, // None=auto, Some(false)=fast, Some(true)=reasoning
    state: State<AppState>,
    app: AppHandle,
) -> Result<String, String> {
    let (auto_model_path, auto_model_name, auto_reasoning_model_path, auto_reasoning_model_name) =
        current_auto_model_info(&state);

    if auto_model_path.is_empty() {
        app.emit("chat-error", "No model file found in C:\\models")
            .ok();
        return Err("No model available".to_string());
    }

    // Resolve or create the conversation synchronously (very fast).
    let conv_id = {
        let db = state.db.lock().map_err(|e| format!("DB lock error: {e}"))?;
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

    // Decide which model to use before spawning the thread.
    // Priority: 1) dev pin  2) per-message override  3) auto classifier.
    let (target_path, target_name) = {
        let dev_id = state
            .dev_model_override
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?
            .clone();

        if let Some(id) = dev_id {
            // Dev pin: find the model by ID and use its path directly.
            let entry = crate::models::MODELS.iter().find(|m| m.id == id);
            let path = entry.and_then(crate::models::installed_path);
            match (entry, path) {
                (Some(e), Some(p)) => (p.to_string_lossy().to_string(), e.display_name.to_string()),
                (Some(e), None) => {
                    // Pinned model is not downloaded — fall through to auto with a warning.
                    log::warn!(
                        "Dev pin '{}' is set but model not found on disk; using auto",
                        e.id
                    );
                    let use_reasoning =
                        !auto_reasoning_model_path.is_empty() && needs_reasoning(&prompt);
                    if use_reasoning {
                        (
                            auto_reasoning_model_path.clone(),
                            auto_reasoning_model_name.clone(),
                        )
                    } else {
                        (auto_model_path.clone(), auto_model_name.clone())
                    }
                }
                _ => (auto_model_path.clone(), auto_model_name.clone()),
            }
        } else {
            let use_reasoning = match model_override {
                Some(forced) => forced && !auto_reasoning_model_path.is_empty(),
                None => !auto_reasoning_model_path.is_empty() && needs_reasoning(&prompt),
            };
            if use_reasoning {
                (
                    auto_reasoning_model_path.clone(),
                    auto_reasoning_model_name.clone(),
                )
            } else {
                (auto_model_path.clone(), auto_model_name.clone())
            }
        }
    };

    let loaded = state.loaded.clone();
    let db_arc = state.db.clone();
    let conv_id_thread = conv_id.clone();
    let stop_flag = state.stop_requested.clone();

    std::thread::spawn(move || {
        let thread_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stop_flag.store(false, Ordering::Relaxed); // reset from any previous stop
                                                       // Load existing history and memories before saving the new message.
            let (history, memories, conversation_summary) = match db_arc.lock() {
                Ok(db) => {
                    let conversation_summary =
                        db.get_conversation_summary(&conv_id_thread).ok().flatten();
                    let history = if let Some(summary) = conversation_summary.as_ref() {
                        db.load_messages_after_id(
                            &conv_id_thread,
                            summary.summarized_through_message_id,
                        )
                        .unwrap_or_else(|_| db.load_messages(&conv_id_thread).unwrap_or_default())
                    } else {
                        db.load_messages(&conv_id_thread).unwrap_or_default()
                    };
                    let memories = db.list_memories().unwrap_or_default();
                    (history, memories, conversation_summary)
                }
                Err(_) => (Vec::new(), Vec::new(), None),
            };

            // Persist the new user message.
            if let Ok(db) = db_arc.lock() {
                db.add_message(&conv_id_thread, "user", &prompt).ok();
            }

            // Recover from a poisoned lock (previous inference panicked while holding it).
            // We reset the model state to None so the next call gets a clean reload.
            let mut guard = match loaded.lock() {
                Ok(g) => g,
                Err(e) => {
                    log::warn!("Loaded-model lock was poisoned by a previous panic — resetting model state");
                    let mut g = e.into_inner();
                    *g = None;
                    g
                }
            };

            // Swap models only when the target file differs from what's loaded.
            let needs_reload = guard.as_ref().map_or(true, |ls| ls.path != target_path);
            if needs_reload {
                if guard.is_some() {
                    log::info!("Swapping model -> {}", target_name);
                    *guard = None; // drop the current model before loading the next
                }
                log::info!("Loading model: {target_path}");
                app.emit("chat-model", &target_name).ok();
                match load_model_with_fallback(&target_path) {
                    Ok(m) => {
                        *guard = Some(LoadedState {
                            path: target_path,
                            inner: m,
                        })
                    }
                    Err(e) => {
                        app.emit("chat-error", &e).ok();
                        return;
                    }
                }
            } else {
                app.emit("chat-model", &target_name).ok();
            }

            let ls = guard.as_ref().unwrap();
            let full_prompt = apply_chat_template_with_summary(
                conversation_summary.as_ref(),
                &history,
                &prompt,
                &memories,
                &ls.path,
            );

            // Tokens that signal end-of-turn across ChatML, Gemma, and Llama formats.
            const STOP_STRINGS: &[&str] = &[
                "<|im_end|>",
                "<|im_start|>", // ChatML (Qwen3, Phi-4)
                "<end_of_turn>",
                "<|end_of_turn|>", // Gemma 4
                "<|eot_id|>",
                "<|end|>",
                "<|endoftext|>", // Llama / Phi-4
            ];

            let mut full_response = String::new();
            // Characters received but not yet emitted — held back in case they turn
            // out to be the beginning of a stop string.
            let mut hold = String::new();

            let result = crate::inference::generate_streaming(
                &ls.inner.backend,
                &ls.inner.model,
                &full_prompt,
                4096,
                |token| {
                    hold.push_str(&token);

                    // If hold already contains a complete stop string — stop immediately
                    // and emit only the content that came before it.
                    if let Some(pos) = earliest_stop_pos(&hold, STOP_STRINGS) {
                        if pos > 0 {
                            let safe = hold[..pos].to_string();
                            full_response.push_str(&safe);
                            app.emit("chat-token", safe).ok();
                        }
                        hold.clear();
                        return false;
                    }

                    // Find the longest trailing substring that could still become a
                    // stop string. This must stay buffered, but only on valid UTF-8
                    // character boundaries so token filtering never panics.
                    let keep_start =
                        partial_stop_suffix_start(&hold, STOP_STRINGS).unwrap_or(hold.len());

                    // Emit the safe portion (everything except the held-back tail).
                    if keep_start > 0 {
                        let safe = hold[..keep_start].to_string();
                        full_response.push_str(&safe);
                        app.emit("chat-token", safe).ok();
                        hold = hold[keep_start..].to_string();
                    }

                    !stop_flag.load(Ordering::Relaxed)
                },
            );

            // Flush whatever is left in the hold buffer (no stop string completed).
            if !hold.is_empty() {
                full_response.push_str(&hold);
                app.emit("chat-token", hold).ok();
            }

            match result {
                Ok(()) => {
                    if let Ok(db) = db_arc.lock() {
                        db.add_message(&conv_id_thread, "assistant", &full_response)
                            .ok();
                    }
                    app.emit("chat-done", ()).ok();
                    spawn_summary_refresh(loaded.clone(), db_arc.clone(), conv_id_thread.clone());
                }
                Err(e) => {
                    app.emit("chat-error", e.to_string()).ok();
                }
            }
        }));

        if let Err(payload) = thread_result {
            let message = panic_message(payload);
            log::error!("Generation thread panicked: {message}");
            app.emit(
                "chat-error",
                format!("Generation crashed unexpectedly: {message}"),
            )
            .ok();
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
pub fn load_conversation(
    id: String,
    state: State<AppState>,
) -> Result<Vec<db::StoredMessage>, String> {
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

// --- Memory commands ---

#[tauri::command]
pub fn list_memories(state: State<AppState>) -> Result<Vec<db::Memory>, String> {
    state
        .db
        .lock()
        .map_err(|e| format!("DB lock error: {e}"))?
        .list_memories()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_memory(content: String, state: State<AppState>) -> Result<db::Memory, String> {
    let content = content.trim().to_string();
    if content.is_empty() {
        return Err("Memory content cannot be empty".to_string());
    }
    state
        .db
        .lock()
        .map_err(|e| format!("DB lock error: {e}"))?
        .add_memory(&content)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_memory(id: i64, state: State<AppState>) -> Result<(), String> {
    state
        .db
        .lock()
        .map_err(|e| format!("DB lock error: {e}"))?
        .delete_memory(id)
        .map_err(|e| e.to_string())
}

// --- File reading ---

#[derive(Serialize)]
pub struct FileMemoryContent {
    pub filename: String,
    pub content: String,
    pub truncated: bool,
}

/// Extract text from a PDF supplied as raw bytes from the browser.
/// Used as a fallback when the OS file path is not accessible from the webview.
#[tauri::command]
pub fn extract_pdf_bytes(bytes: Vec<u8>) -> Result<String, String> {
    let content = pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|e| format!("PDF extraction failed: {e}"))?;

    const MAX_CHARS: usize = 8_000;
    let char_count = content.chars().count();
    if char_count > MAX_CHARS {
        let head: String = content.chars().take(MAX_CHARS).collect();
        Ok(format!(
            "{head}\n\n[… file truncated — showing {MAX_CHARS} of {char_count} characters]"
        ))
    } else {
        Ok(content)
    }
}

/// Read a file from disk and extract its text content.
/// Supports PDF files (text extraction) and all UTF-8 text formats.
/// Content is capped at 8 000 characters so it stays inside the model's context window.
#[tauri::command]
pub fn read_file_for_memory(path: String) -> Result<FileMemoryContent, String> {
    use std::path::Path;

    let path = Path::new(&path);
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());

    let content = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .as_deref()
    {
        Some("pdf") => {
            let bytes = std::fs::read(path).map_err(|e| format!("Failed to read file: {e}"))?;
            pdf_extract::extract_text_from_mem(&bytes)
                .map_err(|e| format!("Failed to extract PDF text: {e}"))?
        }
        _ => std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {e}"))?,
    };

    const MAX_CHARS: usize = 8_000;
    let char_count = content.chars().count();
    let (content, truncated) = if char_count > MAX_CHARS {
        let head: String = content.chars().take(MAX_CHARS).collect();
        (
            format!(
                "{head}\n\n[… file truncated — showing {MAX_CHARS} of {char_count} characters]"
            ),
            true,
        )
    } else {
        (content, false)
    };

    Ok(FileMemoryContent {
        filename,
        content,
        truncated,
    })
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
    let hw = hardware::detect();
    models::list_model_statuses_for_recommended(hw.selected_model.id)
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
