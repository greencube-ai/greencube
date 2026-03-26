#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent_messages;
mod api;
mod capabilities;
mod commandments;
mod commands;
mod competence;
mod config;
mod context;
mod context_budget;
mod db;
mod errors;
mod feedback;
mod goals;
mod identity;
mod idle_thinker;
mod journal;
mod knowledge;
mod memory;
mod metrics;
mod notifications;
mod patterns;
mod permissions;
mod profile;
mod projects;
mod providers;
mod ratings;
mod reflection;
mod sandbox;
mod spawn;
mod state;
mod task_queue;
mod time_sense;
mod token_usage;
mod tool_memory;

use config::config_dir;
use state::AppState;
use std::sync::Arc;
use tauri::Manager; // Required for app.manage()
use tokio::sync::{Mutex, RwLock}; // MUST be tokio::sync, NOT std::sync
use tracing_subscriber::EnvFilter;

fn main() {
    // Initialize tracing — logs to stdout
    let log_dir = config_dir().join("logs");
    let _ = std::fs::create_dir_all(&log_dir);

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("greencube=info,tower_http=info"))
        .init();

    tracing::info!("Starting GreenCube v0.7.0");

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

            // Check Docker
            let docker = tauri::async_runtime::block_on(async {
                match bollard::Docker::connect_with_local_defaults() {
                    Ok(d) => {
                        if d.ping().await.is_ok() {
                            tracing::info!("Docker is available");
                            Some(d)
                        } else {
                            tracing::warn!("Docker is installed but not responding");
                            None
                        }
                    }
                    Err(_) => {
                        tracing::warn!("Docker is not available");
                        None
                    }
                }
            });

            // Bind API server with port fallback (try configured port, then +1..+10)
            let host = config.server.host.clone();
            let port = config.server.port;
            let listener = tauri::async_runtime::block_on(async {
                for p in port..=port + 10 {
                    match tokio::net::TcpListener::bind(format!("{}:{}", host, p)).await {
                        Ok(l) => return l,
                        Err(_) => tracing::warn!("Port {} in use, trying next", p),
                    }
                }
                panic!(
                    "Failed to bind to any port in range {}-{}. Close other processes or change server.port in config.toml.",
                    port,
                    port + 10
                );
            });
            let actual_port = listener.local_addr().expect("bound address").port();
            tracing::info!("API server will listen on {}:{}", host, actual_port);

            // Create shared state
            let state = Arc::new(AppState {
                db: Mutex::new(conn),
                config: RwLock::new(config),
                docker: RwLock::new(docker),
                app_handle: Some(handle),
                actual_port,
            });

            // Store state in Tauri's managed state
            app.manage(state.clone());

            // Spawn axum server with the already-bound listener
            let server_state = state.clone();
            tauri::async_runtime::spawn(async move {
                let router = api::create_router(server_state);
                axum::serve(listener, router)
                    .await
                    .expect("API server crashed");
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
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Graceful shutdown: kill any running GreenCube sandbox containers
                let state = window.state::<Arc<AppState>>();
                tauri::async_runtime::block_on(async {
                    let docker = state.docker.read().await;
                    if let Some(ref docker) = *docker {
                        use bollard::container::{
                            KillContainerOptions, ListContainersOptions, RemoveContainerOptions,
                        };
                        use std::collections::HashMap;
                        let mut filters = HashMap::new();
                        filters.insert("name".to_string(), vec!["greencube-".to_string()]);
                        let opts = ListContainersOptions {
                            all: true,
                            filters,
                            ..Default::default()
                        };
                        if let Ok(containers) = docker.list_containers(Some(opts)).await {
                            for c in containers {
                                if let Some(id) = c.id {
                                    tracing::info!("Cleaning up container: {}", id);
                                    let _ = docker
                                        .kill_container(
                                            &id,
                                            None::<KillContainerOptions<String>>,
                                        )
                                        .await;
                                    let _ = docker
                                        .remove_container(
                                            &id,
                                            Some(RemoveContainerOptions {
                                                force: true,
                                                ..Default::default()
                                            }),
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
