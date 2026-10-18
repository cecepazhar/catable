use catable_core::{ConnectionConfig, DriverKind, GuardVerdict, GuardrailEngine, Vault};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

pub struct AppState {
    pub vault: Result<Vault, String>,
    pub current_connection: Mutex<Option<ConnectionConfig>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            vault: Vault::open().map_err(|e| e.to_string()),
            current_connection: Mutex::new(None),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryEvaluationRequest {
    pub sql: String,
    pub connection_id: Option<String>,
}

#[tauri::command]
pub fn list_connections(state: tauri::State<'_, AppState>) -> Result<Vec<ConnectionConfig>, String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault.list_connections().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_connection(
    state: tauri::State<'_, AppState>,
    config: ConnectionConfig,
) -> Result<(), String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault.save_connection(&config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_connection(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault.delete_connection(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn evaluate_query_safety(
    state: tauri::State<'_, AppState>,
    sql: String,
) -> Result<GuardVerdict, String> {
    let current_guard = state.current_connection.lock().map_err(|e| e.to_string())?;
    let conn = current_guard.clone().unwrap_or(ConnectionConfig {
        id: "default".into(),
        name: "Sandbox".into(),
        driver: DriverKind::Sqlite,
        environment: catable_core::Environment::Development,
        host: "".into(),
        port: 0,
        database: ":memory:".into(),
        user: "".into(),
        password: None,
        read_only: false,
    });
    Ok(GuardrailEngine::evaluate(&sql, &conn))
}

#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
