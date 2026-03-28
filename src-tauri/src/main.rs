#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(dead_code, unused_imports, unused_variables, unused_mut)]

mod agent_messages;
mod api;
mod capabilities;
mod commandments;
mod commands;
mod competence;
mod config;
mod context;
mod context_budget;
mod context_clusters;
mod creature_status;
mod curiosity;
mod db;
mod drives;
mod errors;
mod feedback;
mod fork;
mod goals;
mod identity;
mod idle_thinker;
mod journal;
mod knowledge;
mod memory;
mod metrics;
mod mood;
mod notifications;
mod patterns;
mod permissions;
mod profile;
mod projects;
mod providers;
mod ratings;
mod reflection;
mod relationships;
mod self_verify;
mod spawn;
mod state;
mod task_patterns;
mod task_queue;
mod time_sense;
mod trajectory;
mod token_usage;
mod tool_memory;

use config::config_dir;
use state::AppState;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::{Mutex, RwLock};
use tracing_subscriber::EnvFilter;

fn main() {
    let log_dir = config_dir().join("logs");
    let _ = std::fs::create_dir_all(&log_dir);

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("greencube=info,tower_http=info"))
        .init();

    tracing::info!("Starting GreenCube v1.0.0");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // Ensure data directory exists
            let data_dir = config_dir();
            std::fs::create_dir_all(&data_dir).expect("Failed to create ~/.greencube/");

            // Load config
            let config = config::load_config().expect("Failed to load config");

            // Initialize database
            let db_path = data_dir.join("greencube.db");
            let conn = db::init_database(&db_path).expect("Failed to initialize database");

            // Sync config API key to providers table on startup
            if !config.llm.api_key.is_empty() {
                let _ = conn.execute(
                    "UPDATE providers SET api_key = ?1, api_base_url = ?2, default_model = ?3",
                    rusqlite::params![config.llm.api_key, config.llm.api_base_url, config.llm.default_model],
                );
                tracing::info!("Synced API key from config to providers on startup");
            }

            let port = config.server.port;

            // Create shared state
            let state = Arc::new(AppState {
                db: Mutex::new(conn),
                config: RwLock::new(config.clone()),
                app_handle: Some(handle.clone()),
                actual_port: port,
            });

            app.manage(state.clone());

            // Spawn axum server with port fallback
            let server_state = state.clone();
            let host = config.server.host.clone();

            tauri::async_runtime::spawn(async move {
                let router = api::create_router(server_state);
                for p in port..=(port + 10) {
                    let addr = format!("{}:{}", host, p);
                    match tokio::net::TcpListener::bind(&addr).await {
                        Ok(listener) => {
                            tracing::info!("API server will listen on {}", addr);
                            axum::serve(listener, router).await.expect("API server crashed");
                            return;
                        }
                        Err(_) => {
                            if p < port + 10 {
                                tracing::warn!("Port {} taken, trying {}", p, p + 1);
                            }
                        }
                    }
                }
                tracing::error!("Could not bind to any port {}-{}", port, port + 10);
            });

            // Clean up old queue tasks on startup
            {
                let db_ref = tauri::async_runtime::block_on(async { state.db.lock().await });
                let _ = task_queue::cleanup_old_tasks(&db_ref);
            }

            // Spawn idle thinker background loop
            let idle_state = state.clone();
            tauri::async_runtime::spawn(async move {
                idle_thinker::run_idle_thinker(idle_state).await;
            });

            // Spawn task queue processor
            let queue_state = state.clone();
            tauri::async_runtime::spawn(async move {
                task_queue::run_queue_processor(queue_state).await;
            });

            // System tray with menu
            let open_handle = handle.clone();
            let quit_handle = handle.clone();

            use tauri::menu::MenuBuilder;
            let menu = MenuBuilder::new(app)
                .text("open", "Open GreenCube")
                .separator()
                .text("quit", "Quit")
                .build()?;

            let _tray = tauri::tray::TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("GreenCube — API running")
                .on_menu_event(move |_app, event| {
                    match event.id().as_ref() {
                        "open" => {
                            if let Some(window) = open_handle.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => {
                            quit_handle.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::DoubleClick { .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_agents,
            commands::get_agent,
            commands::create_agent,
            commands::get_episodes,
            commands::get_audit_log,
            commands::get_activity_feed,
            commands::get_config,
            commands::save_config,
            commands::get_docker_status,
            commands::get_server_info,
            commands::reset_app,
            commands::get_agent_lineage,
            commands::debug_spawn,
            commands::get_competence_map,
            commands::get_creature_status,
            commands::rate_response,
            commands::get_approval_rate,
            commands::get_token_usage_today,
            commands::get_agent_messages,
            commands::get_goals,
            commands::get_metrics,
            commands::get_idle_thoughts,
            commands::get_capabilities,
            commands::search_capabilities,
            commands::get_unread_count,
            commands::get_notifications,
            commands::mark_notification_read,
            commands::dismiss_all_notifications,
            commands::get_knowledge,
            commands::get_agent_context,
            commands::set_agent_context,
            commands::get_providers,
            commands::create_provider,
            commands::update_provider,
            commands::delete_provider,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Minimize to tray instead of quitting — proxy stays alive
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
