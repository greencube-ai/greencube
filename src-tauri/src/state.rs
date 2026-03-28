use tokio::sync::{Mutex, RwLock}; // NOT std::sync — Tauri commands are async
use crate::config::AppConfig;

pub struct AppState {
    pub db: Mutex<rusqlite::Connection>,
    pub config: RwLock<AppConfig>,
    pub app_handle: Option<tauri::AppHandle>, // None in tests (no Tauri runtime)
    pub actual_port: u16,
}
