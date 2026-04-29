pub mod commands;
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

            // Detect hardware and find whichever model file is actually on disk.
            let (model_path, model_name) =
                hardware::find_available_model().unwrap_or_else(|| {
                    log::warn!("No model files found in C:\\models");
                    (String::new(), String::from("No model available"))
                });

            log::info!("Selected model: {} ({})", model_name, model_path);

            // Register the state so all commands can access it.
            app.manage(commands::AppState {
                model_name,
                model_path,
                loaded: std::sync::Mutex::new(None),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_model_info,
            commands::send_message,
            commands::send_message_streaming,
            commands::list_models,
            commands::download_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
