pub mod commands;
pub mod db;
pub mod hardware;
pub mod inference;
pub mod models;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Open the conversation history database in the app's data directory.
            let data_dir = app.path().app_data_dir()?;
            let db_path = data_dir.join("conversations.db");
            let db = db::Db::open(&db_path)
                .map_err(|e| format!("Failed to open database: {e}"))?;
            log::info!("Database opened at {}", db_path.display());

            // Detect hardware and find the best model(s) available on disk.
            let (fast, reasoning) = hardware::find_model_pair();

            let (model_path, model_name) = fast.unwrap_or_else(|| {
                log::warn!("No model files found in C:\\models");
                (String::new(), String::from("No model available"))
            });
            let (reasoning_model_path, reasoning_model_name) =
                reasoning.unwrap_or_else(|| (String::new(), String::new()));

            log::info!("Fast model: {} ({})", model_name, model_path);
            if !reasoning_model_path.is_empty() {
                log::info!("Reasoning model: {} ({})", reasoning_model_name, reasoning_model_path);
            }

            app.manage(commands::AppState {
                model_name,
                model_path,
                reasoning_model_name,
                reasoning_model_path,
                loaded: std::sync::Arc::new(std::sync::Mutex::new(None)),
                db: std::sync::Arc::new(std::sync::Mutex::new(db)),
                dev_model_override: std::sync::Arc::new(std::sync::Mutex::new(None)),
                stop_requested: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_model_info,
            commands::is_first_run,
            commands::send_message_streaming,
            commands::list_conversations,
            commands::load_conversation,
            commands::delete_conversation,
            commands::list_models,
            commands::download_model,
            commands::list_memories,
            commands::add_memory,
            commands::delete_memory,
            commands::read_file_for_memory,
            commands::extract_pdf_bytes,
            commands::set_dev_model,
            commands::get_dev_model,
            commands::stop_generation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
