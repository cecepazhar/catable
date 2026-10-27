use catable_core::{
    AiBackendConfig, AiDbaCopilot, AiExplainRequest, AiExplainResponse, AiChatRequest, AiChatResponse,
    CatermError, ConnectionConfig, ConnectionTestResult, DriverKind, GuardVerdict, GuardrailEngine,
    QueryExecutor, QueryResult, QueryRisk, SchemaContext, TableInfo, Vault, WorkspaceInput,
    WorkspaceRecord,
};
use catable_core::billing::EntitlementState;
use catable_core::pro::{
    AccountDetails, DeviceLimit, ProAccount, ProStatus, ProTeamView, SyncOutcome,
};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

async fn run_blocking<F, R>(f: F) -> Result<R, CatermError>
where
    F: FnOnce() -> Result<R, CatermError> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| CatermError::Internal(format!("Task join error: {e}")))?
}

pub struct AppState {
    pub vault: Result<Vault, String>,
    pub current_connection: Mutex<Option<ConnectionConfig>>,
    pub is_vault_unlocked: Mutex<bool>,
    pub is_pro: Mutex<bool>,
    pub sync_connected: Mutex<bool>,
    pub last_sync_at: Mutex<Option<u64>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            vault: Vault::open().map_err(|e| e.to_string()),
            current_connection: Mutex::new(None),
            is_vault_unlocked: Mutex::new(true),
            is_pro: Mutex::new(true),
            sync_connected: Mutex::new(true),
            last_sync_at: Mutex::new(Some(chrono::Utc::now().timestamp() as u64)),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
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
pub fn list_workspaces(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WorkspaceRecord>, String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault.list_workspaces().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_workspace(
    state: tauri::State<'_, AppState>,
    input: WorkspaceInput,
) -> Result<WorkspaceRecord, String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault.save_workspace(&input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_workspace(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault.delete_workspace(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_encrypted_backup(
    state: tauri::State<'_, AppState>,
    passphrase: Option<String>,
    password: Option<String>,
) -> Result<Vec<u8>, String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    let pass = passphrase
        .or(password)
        .ok_or_else(|| "Passphrase is required".to_string())?;
    vault.export_encrypted_backup(&pass).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_encrypted_backup(
    state: tauri::State<'_, AppState>,
    data: Vec<u8>,
    passphrase: Option<String>,
    password: Option<String>,
) -> Result<usize, String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    let pass = passphrase
        .or(password)
        .ok_or_else(|| "Passphrase is required".to_string())?;
    vault.import_encrypted_backup(&data, &pass).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_json_backup(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault.export_json_backup().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_json_backup(
    state: tauri::State<'_, AppState>,
    json_str: String,
) -> Result<usize, String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault.import_json_backup(&json_str).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn evaluate_query_safety(
    state: tauri::State<'_, AppState>,
    sql: String,
    config: Option<ConnectionConfig>,
) -> Result<GuardVerdict, String> {
    let mut conn: Option<ConnectionConfig> = None;
    if let Some(cfg) = config {
        conn = Some(cfg);
    }
    if conn.is_none() {
        let current_guard = state.current_connection.lock().map_err(|e| e.to_string())?;
        conn = current_guard.clone();
    }
    let conn = conn.unwrap_or(ConnectionConfig {
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
        ssh_tunnel: None,
        group_name: None,
    });
    Ok(GuardrailEngine::evaluate(&sql, &conn))
}

#[tauri::command]
pub fn test_ssh_tunnel(config: catable_core::SshTunnelConfig) -> Result<bool, String> {
    catable_core::test_ssh_tunnel(&config)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn test_database_connection(config: ConnectionConfig) -> Result<ConnectionTestResult, String> {
    Ok(QueryExecutor::test_connection(&config))
}

#[tauri::command]
pub fn list_database_tables(config: ConnectionConfig) -> Result<Vec<TableInfo>, String> {
    QueryExecutor::list_tables(&config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn execute_database_query(config: ConnectionConfig, sql: String) -> Result<QueryResult, String> {
    Ok(QueryExecutor::execute_query(&config, &sql))
}

#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub fn is_vault_unlocked(state: tauri::State<'_, AppState>) -> bool {
    *state.is_vault_unlocked.lock().unwrap_or_else(|e| e.into_inner())
}

#[tauri::command]
pub fn is_vault_initialized() -> Result<bool, String> {
    catable_core::is_vault_initialized().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn validate_vault_password(
    state: tauri::State<'_, AppState>,
    password: String,
) -> Result<bool, String> {
    let ok = catable_core::validate_vault_password(&password).map_err(|e| e.to_string())?;
    *state.is_vault_unlocked.lock().unwrap_or_else(|e| e.into_inner()) = true;
    catable_core::vault::mark_unlocked();
    Ok(ok)
}

#[tauri::command]
pub fn reset_vault(state: tauri::State<'_, AppState>) -> Result<(), String> {
    catable_core::reset_vault().map_err(|e| e.to_string())?;
    *state.is_vault_unlocked.lock().unwrap_or_else(|e| e.into_inner()) = false;
    catable_core::vault::mark_locked();
    Ok(())
}

#[tauri::command]
pub fn lock_vault(state: tauri::State<'_, AppState>) -> Result<(), String> {
    *state.is_vault_unlocked.lock().unwrap_or_else(|e| e.into_inner()) = false;
    catable_core::vault::mark_locked();
    Ok(())
}

#[tauri::command]
pub fn change_master_password(
    state: tauri::State<'_, AppState>,
    old_password: String,
    new_password: String,
) -> Result<(), String> {
    catable_core::change_master_password(&old_password, &new_password)
        .map_err(|e| e.to_string())?;
    *state.is_vault_unlocked.lock().unwrap_or_else(|e| e.into_inner()) = true;
    catable_core::vault::mark_unlocked();
    Ok(())
}

#[tauri::command]
pub fn window_minimize(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
pub fn window_maximize(window: tauri::Window) {
    if let Ok(is_max) = window.is_maximized() {
        if is_max {
            let _ = window.unmaximize();
        } else {
            let _ = window.maximize();
        }
    } else {
        let _ = window.maximize();
    }
}

#[tauri::command]
pub fn window_close(window: tauri::Window) {
    let _ = window.close();
}

#[tauri::command]
pub fn window_start_dragging(window: tauri::Window) {
    let _ = window.start_dragging();
}

// ---------------------------------------------------------------------------
// Pro Tier & Billing IPC
// ---------------------------------------------------------------------------

/// What the blast-radius simulator reports about a SQL statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlastRadiusResult {
    pub risk: QueryRisk,
    pub tables_affected: Vec<String>,
    pub estimated_rows: u64,
    pub is_destructive: bool,
    pub requires_confirmation: bool,
    pub shield_action: String,
    pub explanation: String,
}

#[tauri::command]
pub async fn pro_status(state: tauri::State<'_, AppState>) -> Result<ProStatus, CatermError> {
    let st = run_blocking(catable_core::pro::status).await?;
    *state.is_pro.lock().unwrap_or_else(|e| e.into_inner()) = st.is_pro;
    *state.sync_connected.lock().unwrap_or_else(|e| e.into_inner()) = st.sync_connected;
    if let Some(ts) = st.last_sync_at {
        *state.last_sync_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(ts as u64);
    }
    Ok(st)
}

#[tauri::command]
pub async fn pro_server_available() -> Result<bool, CatermError> {
    run_blocking(|| Ok(catable_core::pro::server_available())).await
}

#[tauri::command]
pub async fn pro_register(
    email: String,
    password: String,
    name: String,
    locale: String,
) -> Result<(), CatermError> {
    run_blocking(move || catable_core::pro::register(&email, &password, &name, &locale)).await
}

#[tauri::command]
pub async fn pro_resend_verification(email: String) -> Result<(), CatermError> {
    run_blocking(move || catable_core::pro::resend_verification(&email)).await
}

#[tauri::command]
pub async fn pro_forgot_password(email: String, locale: String) -> Result<(), CatermError> {
    run_blocking(move || catable_core::pro::forgot_password(&email, &locale)).await
}

#[tauri::command]
pub async fn pro_login(
    email: String,
    password: String,
) -> Result<ProAccount, CatermError> {
    run_blocking(move || catable_core::pro::login(&email, &password)).await
}

#[tauri::command]
pub async fn pro_commit_pending() -> Result<bool, CatermError> {
    run_blocking(catable_core::pro::commit_pending).await
}

#[tauri::command]
pub async fn pro_sync(state: tauri::State<'_, AppState>) -> Result<SyncOutcome, CatermError> {
    let outcome = run_blocking(catable_core::pro::sync).await?;
    *state.is_pro.lock().unwrap_or_else(|e| e.into_inner()) = outcome.status.is_pro;
    *state.sync_connected.lock().unwrap_or_else(|e| e.into_inner()) = outcome.status.sync_connected;
    if let Some(ts) = outcome.status.last_sync_at {
        *state.last_sync_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(ts as u64);
    }
    Ok(outcome)
}

#[tauri::command]
pub async fn pro_start_trial(state: tauri::State<'_, AppState>) -> Result<SyncOutcome, CatermError> {
    let outcome = run_blocking(catable_core::pro::start_trial).await?;
    *state.is_pro.lock().unwrap_or_else(|e| e.into_inner()) = outcome.status.is_pro;
    *state.sync_connected.lock().unwrap_or_else(|e| e.into_inner()) = outcome.status.sync_connected;
    if let Some(ts) = outcome.status.last_sync_at {
        *state.last_sync_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(ts as u64);
    }
    Ok(outcome)
}

#[tauri::command]
pub async fn pro_account() -> Result<AccountDetails, CatermError> {
    run_blocking(catable_core::pro::account_details).await
}

#[tauri::command]
pub async fn pro_revoke_device(device_id: String) -> Result<(), CatermError> {
    run_blocking(move || catable_core::pro::revoke_device(&device_id)).await
}

#[tauri::command]
pub async fn pro_logout(state: tauri::State<'_, AppState>) -> Result<(), CatermError> {
    *state.is_pro.lock().unwrap_or_else(|e| e.into_inner()) = false;
    *state.sync_connected.lock().unwrap_or_else(|e| e.into_inner()) = false;
    run_blocking(catable_core::pro::logout).await
}

#[tauri::command]
pub async fn pro_activate_license(license_key: String) -> Result<ProStatus, CatermError> {
    run_blocking(move || catable_core::pro::activate_license_key(&license_key)).await
}

#[tauri::command]
pub async fn pro_team() -> Result<ProTeamView, CatermError> {
    run_blocking(catable_core::pro::team).await
}

#[tauri::command]
pub async fn pro_team_invite(email: String, locale: String) -> Result<ProTeamView, CatermError> {
    run_blocking(move || catable_core::pro::team_invite(&email, &locale)).await
}

#[tauri::command]
pub async fn pro_team_cancel_invite(invite_id: String) -> Result<ProTeamView, CatermError> {
    run_blocking(move || catable_core::pro::team_cancel_invite(&invite_id)).await
}

#[tauri::command]
pub async fn pro_team_remove_member(account_id: String) -> Result<ProTeamView, CatermError> {
    run_blocking(move || catable_core::pro::team_remove_member(&account_id)).await
}

#[tauri::command]
pub async fn pro_team_accept(invitation_id: String) -> Result<ProTeamView, CatermError> {
    run_blocking(move || catable_core::pro::team_accept(&invitation_id)).await
}

#[tauri::command]
pub async fn pro_team_decline(invitation_id: String) -> Result<ProTeamView, CatermError> {
    run_blocking(move || catable_core::pro::team_decline(&invitation_id)).await
}

#[tauri::command]
pub async fn pro_team_leave() -> Result<ProTeamView, CatermError> {
    run_blocking(catable_core::pro::team_leave).await
}

// Teams aliases
#[tauri::command]
pub async fn teams_view() -> Result<ProTeamView, CatermError> {
    run_blocking(catable_core::pro::team).await
}

#[tauri::command]
pub async fn teams_invite(email: String, locale: String) -> Result<ProTeamView, CatermError> {
    run_blocking(move || catable_core::pro::team_invite(&email, &locale)).await
}

#[tauri::command]
pub async fn teams_cancel_invite(invite_id: String) -> Result<ProTeamView, CatermError> {
    run_blocking(move || catable_core::pro::team_cancel_invite(&invite_id)).await
}

#[tauri::command]
pub async fn teams_remove_member(account_id: String) -> Result<ProTeamView, CatermError> {
    run_blocking(move || catable_core::pro::team_remove_member(&account_id)).await
}

#[tauri::command]
pub async fn teams_accept(invitation_id: String) -> Result<ProTeamView, CatermError> {
    run_blocking(move || catable_core::pro::team_accept(&invitation_id)).await
}

#[tauri::command]
pub async fn teams_decline(invitation_id: String) -> Result<ProTeamView, CatermError> {
    run_blocking(move || catable_core::pro::team_decline(&invitation_id)).await
}

#[tauri::command]
pub async fn teams_leave() -> Result<ProTeamView, CatermError> {
    run_blocking(catable_core::pro::team_leave).await
}

// Billing IPC
#[tauri::command]
pub async fn billing_get_effective_state() -> Result<EntitlementState, CatermError> {
    run_blocking(catable_core::billing::get_effective_state).await
}

#[tauri::command]
pub async fn billing_request_otp(email: String) -> Result<(), CatermError> {
    run_blocking(move || catable_core::billing::request_otp(&email)).await
}

#[tauri::command]
pub async fn billing_verify_otp(email: String, otp_code: String) -> Result<(), CatermError> {
    run_blocking(move || catable_core::billing::verify_otp(&email, &otp_code)).await
}

#[tauri::command]
pub async fn billing_activate_license(license_key: String) -> Result<ProStatus, CatermError> {
    run_blocking(move || catable_core::billing::activate_license(&license_key)).await
}

#[tauri::command]
pub async fn billing_create_checkout(plan_id: String) -> Result<catable_core::billing::CheckoutResult, CatermError> {
    run_blocking(move || catable_core::billing::create_checkout(&plan_id)).await
}

#[tauri::command]
pub async fn billing_poll_checkout(checkout_id: String) -> Result<String, CatermError> {
    run_blocking(move || catable_core::billing::poll_checkout(&checkout_id)).await
}

#[tauri::command]
pub async fn billing_sign_out(state: tauri::State<'_, AppState>) -> Result<(), CatermError> {
    *state.is_pro.lock().unwrap_or_else(|e| e.into_inner()) = false;
    *state.sync_connected.lock().unwrap_or_else(|e| e.into_inner()) = false;
    run_blocking(catable_core::billing::sign_out).await
}

#[tauri::command]
pub async fn pro_sync_vault(state: tauri::State<'_, AppState>) -> Result<u64, CatermError> {
    let is_pro = run_blocking(|| Ok(catable_core::pro::is_pro())).await?;
    if !is_pro {
        return Err(CatermError::ProRequired(
            "Pro license is required to sync the vault.".into(),
        ));
    }

    let unlocked = *state.is_vault_unlocked.lock().unwrap_or_else(|e| e.into_inner());
    if !unlocked {
        return Err(CatermError::Vault("Vault is locked.".into()));
    }

    let now = chrono::Utc::now().timestamp() as u64;
    *state.last_sync_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(now);
    *state.sync_connected.lock().unwrap_or_else(|e| e.into_inner()) = true;
    Ok(now)
}

#[tauri::command]
pub async fn pro_ssh_tunnel(
    host_id: String,
    target_port: u16,
    _state: tauri::State<'_, AppState>,
) -> Result<u16, CatermError> {
    let _ = target_port;
    let is_pro = run_blocking(|| Ok(catable_core::pro::is_pro())).await?;
    if !is_pro {
        return Err(CatermError::ProRequired(
            "Pro license is required to create SSH tunnels.".into(),
        ));
    }
    if host_id.is_empty() {
        return Err(CatermError::Tunnel("Host id must not be empty.".into()));
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| CatermError::Internal(format!("Failed to allocate a local port: {e}")))?;
    let local_port = listener
        .local_addr()
        .ok()
        .map(|a| a.port());
    drop(listener);
    let local_port = local_port.ok_or(CatermError::Tunnel(
        "No local port could be allocated.".into(),
    ))?;
    Ok(local_port)
}

/// Dry-Run simulator for the Production Blast-Radius Shield (PRD §3.5).
#[tauri::command]
pub fn evaluate_query_blast_radius(
    sql: String,
    conn_config: Option<ConnectionConfig>,
    state: tauri::State<'_, AppState>,
) -> BlastRadiusResult {
    let mut conn: Option<ConnectionConfig> = conn_config;
    if conn.is_none() {
        let current = state.current_connection.lock().unwrap_or_else(|e| e.into_inner());
        conn = current.clone();
    }
    let conn = conn.unwrap_or(ConnectionConfig {
        id: "default".into(),
        name: "Sandbox".into(),
        driver: DriverKind::Sqlite,
        environment: catable_core::Environment::Development,
        host: String::new(),
        port: 0,
        database: ":memory:".into(),
        user: String::new(),
        password: None,
        read_only: false,
        ssh_tunnel: None,
        group_name: None,
    });

    let upper = sql.to_uppercase();
    let trimmed = upper.trim();
    let tables_affected = extract_tables(trimmed);
    let is_read = trimmed.starts_with("SELECT")
        || trimmed.starts_with("EXPLAIN")
        || trimmed.starts_with("SHOW")
        || trimmed.starts_with("DESCRIBE")
        || trimmed.starts_with("PRAGMA")
        || trimmed.starts_with("WITH");

    let unbounded_delete = trimmed.starts_with("DELETE FROM") && !trimmed.contains("WHERE");
    let unbounded_update = trimmed.starts_with("UPDATE") && !trimmed.contains("WHERE");

    // 1. Read-only gate
    if conn.read_only && !is_read {
        return BlastRadiusResult {
            risk: QueryRisk::Blocked,
            tables_affected: tables_affected.clone(),
            estimated_rows: 0,
            is_destructive: true,
            requires_confirmation: false,
            shield_action: "Blocked".into(),
            explanation: "Connection is read-only; write queries are disallowed.".into(),
        };
    }

    // 2. Destructive DDL (DROP / TRUNCATE)
    let destructive_ddl = trimmed.starts_with("DROP TABLE")
        || trimmed.starts_with("DROP DATABASE")
        || trimmed.starts_with("TRUNCATE");
    if destructive_ddl {
        if conn.is_production() {
            return BlastRadiusResult {
                risk: QueryRisk::Blocked,
                tables_affected: tables_affected.clone(),
                estimated_rows: u64::MAX,
                is_destructive: true,
                requires_confirmation: false,
                shield_action: "Blocked".into(),
                explanation: "Destructive DDL (DROP/TRUNCATE) is strictly blocked on Production connections.".into(),
            };
        }
        return BlastRadiusResult {
            risk: QueryRisk::Destructive,
            tables_affected: tables_affected.clone(),
            estimated_rows: u64::MAX,
            is_destructive: true,
            requires_confirmation: true,
            shield_action: "Warn".into(),
            explanation: "Destructive DDL: will permanently delete tables or data.".into(),
        };
    }

    // 3. Unbounded DELETE / UPDATE
    if unbounded_delete || unbounded_update {
        if conn.is_production() {
            return BlastRadiusResult {
                risk: QueryRisk::Blocked,
                tables_affected: tables_affected.clone(),
                estimated_rows: u64::MAX,
                is_destructive: true,
                requires_confirmation: false,
                shield_action: "Blocked".into(),
                explanation: "Unbounded DELETE/UPDATE without a WHERE clause is blocked on Production connections.".into(),
            };
        }
        return BlastRadiusResult {
            risk: QueryRisk::Destructive,
            tables_affected: tables_affected.clone(),
            estimated_rows: u64::MAX,
            is_destructive: true,
            requires_confirmation: true,
            shield_action: "Warn".into(),
            explanation: "Unbounded DELETE/UPDATE affects all rows in the target table.".into(),
        };
    }

    // 4. Targeted mutations — Warning on Production
    let targeted_mutation = trimmed.starts_with("DELETE FROM") || trimmed.starts_with("UPDATE");
    if targeted_mutation && conn.is_production() {
        return BlastRadiusResult {
            risk: QueryRisk::Warning,
            tables_affected: tables_affected.clone(),
            estimated_rows: 0,
            is_destructive: false,
            requires_confirmation: true,
            shield_action: "Warn".into(),
            explanation: "Data modification: verify WHERE clause conditions before executing on production.".into(),
        };
    }

    // 5. Schema alterations
    let schema_alter = trimmed.starts_with("ALTER TABLE")
        || trimmed.starts_with("DROP INDEX")
        || trimmed.starts_with("DROP VIEW");
    if schema_alter {
        return BlastRadiusResult {
            risk: QueryRisk::Warning,
            tables_affected: tables_affected.clone(),
            estimated_rows: 0,
            is_destructive: false,
            requires_confirmation: true,
            shield_action: "Warn".into(),
            explanation: "Schema modification: verify target object definition before applying.".into(),
        };
    }

    BlastRadiusResult {
        risk: QueryRisk::Safe,
        tables_affected,
        estimated_rows: 0,
        is_destructive: false,
        requires_confirmation: false,
        shield_action: "Allow".into(),
        explanation: "Read-only query; no blast radius detected.".into(),
    }
}

fn extract_tables(sql: &str) -> Vec<String> {
    let mut tables = Vec::new();
    let words = sql.split(|c: char| c.is_whitespace() || c == '(' || c == ')');
    let mut last_keyword = false;
    for w in words {
        if w.is_empty() {
            continue;
        }
        if w == "FROM" || w == "INTO" || w == "UPDATE" || w == "TABLE" {
            last_keyword = true;
            continue;
        }
        if last_keyword {
            let cleaned: String = w
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !cleaned.is_empty() {
                tables.push(cleaned.to_lowercase());
                last_keyword = false;
            }
        }
    }
    tables.dedup();
    tables
}

// ---------------------------------------------------------------------------
// AI DBA Copilot IPC
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn ai_chat(
    prompt: String,
    connection: Option<ConnectionConfig>,
    backend: Option<AiBackendConfig>,
) -> Result<AiChatResponse, String> {
    tokio::task::spawn_blocking(move || {
        let be = backend.unwrap_or_default();
        let schema = connection
            .as_ref()
            .and_then(|c| SchemaContext::from_connection(c).ok());

        let req = AiChatRequest { prompt, schema };
        AiDbaCopilot::chat(&be, &req).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn ai_explain(
    sql: String,
    connection: Option<ConnectionConfig>,
    backend: Option<AiBackendConfig>,
) -> Result<AiExplainResponse, String> {
    tokio::task::spawn_blocking(move || {
        let be = backend.unwrap_or_default();
        let schema = connection
            .as_ref()
            .and_then(|c| SchemaContext::from_connection(c).ok());

        let req = AiExplainRequest { sql, schema };
        AiDbaCopilot::explain(&be, &req).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

