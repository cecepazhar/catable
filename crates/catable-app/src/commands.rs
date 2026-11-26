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
    /// Open database sessions (tunnel + connection pool per connection).
    pub sessions: catable_core::SessionManager,
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
            sessions: catable_core::SessionManager::new(),
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
pub fn list_groups(
    state: tauri::State<'_, AppState>,
    workspace_id: Option<String>,
) -> Result<Vec<catable_core::PersonalizedGroupRecord>, String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault.list_groups(workspace_id.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_group(
    state: tauri::State<'_, AppState>,
    input: catable_core::PersonalizedGroupInput,
) -> Result<catable_core::PersonalizedGroupRecord, String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault.save_group(&input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_group(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault.delete_group(&id).map_err(|e| e.to_string())
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
        connect_timeout_secs: None,
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
        workspace_id: None,
    });
    Ok(GuardrailEngine::evaluate(&sql, &conn))
}

// DB/network commands are `async` so Tauri runs them on the async runtime
// instead of the main thread; blocking work (ssh2, SQLite) is moved onto the
// blocking pool inside the core.
#[tauri::command]
pub async fn test_ssh_tunnel(
    config: catable_core::SshTunnelConfig,
) -> Result<catable_core::SshTestResult, String> {
    tokio::task::spawn_blocking(move || catable_core::test_ssh_tunnel_report(&config))
        .await
        .map_err(|e| format!("SSH test task failed: {e}"))
}

/// The user approved this server's key fingerprint.
#[tauri::command]
pub fn ssh_trust_host(host: String, port: u16, key_type: String, fingerprint: String) -> Result<(), String> {
    catable_core::ssh_hosts::trust(&host, port, &key_type, &fingerprint).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ssh_forget_host(host: String, port: u16) -> Result<(), String> {
    catable_core::ssh_hosts::forget(&host, port).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_database_connection(
    config: ConnectionConfig,
) -> Result<ConnectionTestResult, String> {
    Ok(QueryExecutor::test_connection_async(&config).await)
}

// ---- Sessions -------------------------------------------------------------
// The frontend opens a session by saved-connection id; the decrypted config
// (password, SSH secrets) stays in Rust. Every later call carries only the
// opaque session id.

fn resolve_connection(state: &AppState, id: &str) -> Result<ConnectionConfig, String> {
    if id == catable_core::SANDBOX_CONNECTION_ID {
        return Ok(ConnectionConfig::sandbox());
    }
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault
        .list_connections()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("Connection '{id}' was not found in the vault."))
}

#[tauri::command]
pub async fn session_open(
    state: tauri::State<'_, AppState>,
    connection_id: String,
) -> Result<catable_core::SessionInfo, String> {
    let cfg = resolve_connection(&state, &connection_id)?;
    state.sessions.open(cfg).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_close(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<bool, String> {
    Ok(state.sessions.close(&session_id).await)
}

#[tauri::command]
pub async fn session_execute(
    state: tauri::State<'_, AppState>,
    session_id: String,
    sql: String,
) -> Result<QueryResult, String> {
    let session = state.sessions.get(&session_id).await.map_err(|e| e.to_string())?;
    Ok(session.execute(&sql).await)
}

#[tauri::command]
pub async fn session_list_tables(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<Vec<TableInfo>, String> {
    let session = state.sessions.get(&session_id).await.map_err(|e| e.to_string())?;
    session.list_tables().await.map_err(|e| e.to_string())
}

/// One page of a table with server-side filter / sort / paging (+ exact total).
#[tauri::command]
pub async fn session_browse(
    state: tauri::State<'_, AppState>,
    session_id: String,
    request: catable_core::BrowseRequest,
) -> Result<catable_core::BrowseResult, String> {
    let session = state.sessions.get(&session_id).await.map_err(|e| e.to_string())?;
    session.browse(&request).await.map_err(|e| e.to_string())
}

/// The SELECT the browse request would run, without running it.
#[tauri::command]
pub async fn session_build_select(
    state: tauri::State<'_, AppState>,
    session_id: String,
    request: catable_core::BrowseRequest,
) -> Result<String, String> {
    let session = state.sessions.get(&session_id).await.map_err(|e| e.to_string())?;
    session.build_select_sql(&request).map_err(|e| e.to_string())
}

/// Columns, keys, foreign keys, indexes and DDL of a table.
#[tauri::command]
pub async fn session_describe_table(
    state: tauri::State<'_, AppState>,
    session_id: String,
    table: String,
    schema: Option<String>,
) -> Result<catable_core::TableStructure, String> {
    let session = state.sessions.get(&session_id).await.map_err(|e| e.to_string())?;
    session
        .describe_structure(&table, schema.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// The SQL a staged change set will run (shown in the commit preview). Touches nothing.
#[tauri::command]
pub async fn session_preview_changes(
    state: tauri::State<'_, AppState>,
    session_id: String,
    changes: catable_core::RowChangeSet,
) -> Result<Vec<String>, String> {
    let session = state.sessions.get(&session_id).await.map_err(|e| e.to_string())?;
    session.preview_changes(&changes).map_err(|e| e.to_string())
}

/// Applies a staged change set in one transaction; any failure rolls everything back.
#[tauri::command]
pub async fn session_apply_changes(
    state: tauri::State<'_, AppState>,
    session_id: String,
    changes: catable_core::RowChangeSet,
) -> Result<catable_core::ApplyResult, String> {
    let session = state.sessions.get(&session_id).await.map_err(|e| e.to_string())?;
    session.apply_changes(&changes).await.map_err(|e| e.to_string())
}

/// Round-trip time (ms) of a trivial query on the session.
#[tauri::command]
pub async fn session_ping(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<u64, String> {
    let session = state.sessions.get(&session_id).await.map_err(|e| e.to_string())?;
    session.ping().await.map_err(|e| e.to_string())
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
        connect_timeout_secs: None,
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
        workspace_id: None,
    });

    // One source of truth: the parser-based guardrail decides the risk, so this dry-run cannot
    // disagree with (or be weaker than) what the executor actually enforces.
    let verdict = GuardrailEngine::evaluate(&sql, &conn);
    let tables_affected = extract_tables(sql.to_uppercase().trim());
    let reason = verdict.reason.clone().unwrap_or_default();
    let unbounded = reason.to_lowercase().contains("unbounded");

    match verdict.risk {
        QueryRisk::Blocked => BlastRadiusResult {
            risk: QueryRisk::Blocked,
            tables_affected,
            estimated_rows: if unbounded { u64::MAX } else { 0 },
            is_destructive: true,
            requires_confirmation: false,
            shield_action: "Blocked".into(),
            explanation: reason,
        },
        QueryRisk::Destructive => BlastRadiusResult {
            risk: QueryRisk::Destructive,
            tables_affected,
            estimated_rows: u64::MAX,
            is_destructive: true,
            requires_confirmation: true,
            shield_action: "Warn".into(),
            explanation: reason,
        },
        QueryRisk::Warning => BlastRadiusResult {
            risk: QueryRisk::Warning,
            tables_affected,
            estimated_rows: 0,
            is_destructive: false,
            requires_confirmation: true,
            shield_action: "Warn".into(),
            explanation: reason,
        },
        QueryRisk::Safe => BlastRadiusResult {
            risk: QueryRisk::Safe,
            tables_affected,
            estimated_rows: 0,
            is_destructive: false,
            requires_confirmation: false,
            shield_action: "Allow".into(),
            explanation: "Read-only query; no blast radius detected.".into(),
        },
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


// ===================== DB Service Engine Manager (DBngin-style) =====================
use catable_core::service::{self as svc, DbService, EngineType, EngineVersionInfo, ProbeResult, ServiceProgress};
use catable_core::Environment;
use tauri::Emitter;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SvcProgressEvent {
    id: String,
    #[serde(flatten)]
    progress: ServiceProgress,
}

fn emit_progress(app: &tauri::AppHandle, id: &str, progress: ServiceProgress) {
    let _ = app.emit("svc-progress", SvcProgressEvent { id: id.to_string(), progress });
}

fn parse_engine(engine: &str) -> Result<EngineType, String> {
    EngineType::parse(engine).ok_or_else(|| format!("Unknown engine: {engine}"))
}

async fn blocking<R: Send + 'static>(
    f: impl FnOnce() -> Result<R, String> + Send + 'static,
) -> Result<R, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("Task join error: {e}"))?
}

#[tauri::command]
pub async fn svc_list() -> Result<Vec<DbService>, String> {
    blocking(|| svc::list().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn svc_get(service_id: String) -> Result<DbService, String> {
    blocking(move || svc::get(&service_id).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub fn svc_engine_versions() -> Vec<EngineVersionInfo> {
    svc::engine_versions()
}

#[tauri::command]
pub fn svc_available_versions(engine: String) -> Result<Vec<String>, String> {
    Ok(parse_engine(&engine)?.versions().into_iter().map(String::from).collect())
}

#[tauri::command]
pub async fn svc_suggest_port(engine: String) -> Result<u16, String> {
    let eng = parse_engine(&engine)?;
    blocking(move || Ok(svc::suggest_port(eng))).await
}

#[tauri::command]
pub async fn svc_create(
    engine: String,
    version: String,
    name: String,
    preferred_port: Option<u16>,
    auto_start: Option<bool>,
) -> Result<DbService, String> {
    let eng = parse_engine(&engine)?;
    let port = preferred_port.unwrap_or_else(|| eng.default_port());
    blocking(move || {
        svc::create_service(eng, &version, &name, port, auto_start.unwrap_or(false))
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn svc_start(app: tauri::AppHandle, service_id: String) -> Result<DbService, String> {
    blocking(move || {
        let mut cb = |p: ServiceProgress| emit_progress(&app, &service_id, p);
        svc::start(&service_id, &mut cb).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn svc_restart(app: tauri::AppHandle, service_id: String) -> Result<DbService, String> {
    blocking(move || {
        let mut cb = |p: ServiceProgress| emit_progress(&app, &service_id, p);
        svc::restart(&service_id, &mut cb).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn svc_stop(service_id: String) -> Result<(), String> {
    blocking(move || svc::stop(&service_id).map(|_| ()).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn svc_stop_all() -> Result<usize, String> {
    blocking(|| Ok(svc::stop_all())).await
}

#[tauri::command]
pub async fn svc_update(
    service_id: String,
    name: Option<String>,
    port: Option<u16>,
    auto_start: Option<bool>,
) -> Result<DbService, String> {
    blocking(move || svc::update(&service_id, name, port, auto_start).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn svc_remove(service_id: String, delete_data: Option<bool>) -> Result<(), String> {
    blocking(move || svc::remove(&service_id, delete_data.unwrap_or(true)).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn svc_logs(service_id: String, lines: Option<usize>) -> Result<String, String> {
    blocking(move || svc::logs(&service_id, lines.unwrap_or(200)).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn svc_reveal_data_dir(service_id: String) -> Result<(), String> {
    blocking(move || {
        let s = svc::get(&service_id).map_err(|e| e.to_string())?;
        let _ = std::fs::create_dir_all(&s.data_dir);
        #[cfg(windows)]
        let opener = "explorer";
        #[cfg(target_os = "macos")]
        let opener = "open";
        #[cfg(all(not(windows), not(target_os = "macos")))]
        let opener = "xdg-open";
        std::process::Command::new(opener)
            .arg(&s.data_dir)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
}

/// Connection settings CATable Studio uses for a local service and one of its databases.
fn service_connection(s: &DbService, database: Option<&str>) -> ConnectionConfig {
    let default_db = match s.engine {
        EngineType::Postgres if s.database.is_empty() => "postgres".to_string(),
        EngineType::Redis => "0".to_string(),
        _ => s.database.clone(),
    };
    let db = database.map(str::trim).filter(|d| !d.is_empty()).map(str::to_string).unwrap_or(default_db.clone());
    let driver = match s.engine {
        EngineType::Postgres => DriverKind::Postgres,
        EngineType::Mysql => DriverKind::Mysql,
        EngineType::Mariadb => DriverKind::MariaDb,
        EngineType::Redis => DriverKind::Redis,
    };
    let name = if db == default_db { s.name.clone() } else { format!("{} · {}", s.name, db) };
    ConnectionConfig {
        // One saved connection per service + database, so reopening updates it instead of piling up.
        id: format!("svc-{}-{}", s.id, db),
        name,
        driver,
        environment: Environment::Development,
        host: "127.0.0.1".into(),
        port: s.port,
        database: db,
        user: s.default_user().to_string(),
        password: None,
        read_only: false,
        ssh_tunnel: None,
        group_name: Some("Local Services".into()),
        workspace_id: None,
        connect_timeout_secs: Some(5),
    }
}

async fn running_service(service_id: String) -> Result<DbService, String> {
    let s = blocking(move || svc::get(&service_id).map_err(|e| e.to_string())).await?;
    if !svc::is_port_open(s.port, std::time::Duration::from_millis(300)) {
        return Err(format!("{} is not running. Start it first.", s.name));
    }
    Ok(s)
}

/// Saves (or refreshes) the vault connection for a service database and returns it, ready for
/// `dbStore.connect`. Sessions look connections up in the vault, so it must be stored first.
#[tauri::command]
pub async fn svc_open_in_studio(
    state: tauri::State<'_, AppState>,
    service_id: String,
    database: Option<String>,
) -> Result<ConnectionConfig, String> {
    let s = blocking(move || svc::get(&service_id).map_err(|e| e.to_string())).await?;
    let cfg = service_connection(&s, database.as_deref());
    let vault = state.vault.as_ref().map_err(|e| e.clone())?;
    vault.save_connection(&cfg).map_err(|e| e.to_string())?;
    Ok(cfg)
}

const SYSTEM_DATABASES: &[&str] = &["information_schema", "mysql", "performance_schema", "sys"];

/// The same server, pointed at a database every user can reach for catalog work.
fn admin_variant(cfg: &ConnectionConfig) -> ConnectionConfig {
    let database = match cfg.driver {
        DriverKind::Postgres => "postgres".to_string(),
        DriverKind::Mysql | DriverKind::MariaDb => String::new(),
        _ => cfg.database.clone(),
    };
    ConnectionConfig { database, ..cfg.clone() }
}

/// Runs a catalog statement on the admin database, falling back to the configured one
/// (some hosted PostgreSQL users may not connect to `postgres`).
async fn run_admin(cfg: &ConnectionConfig, sql: &str) -> Result<QueryResult, String> {
    let first = QueryExecutor::execute_query_async(&admin_variant(cfg), sql).await;
    let res = match &first.error {
        Some(e) if cfg.driver == DriverKind::Postgres && !cfg.database.is_empty() && e.contains("does not exist") => {
            QueryExecutor::execute_query_async(cfg, sql).await
        }
        _ => first,
    };
    match res.error {
        Some(e) => Err(e),
        None => Ok(res),
    }
}

/// User databases on the server a connection points at (system databases left out).
async fn list_databases_for(cfg: &ConnectionConfig) -> Result<Vec<String>, String> {
    let sql = match cfg.driver {
        DriverKind::Postgres => {
            "SELECT datname FROM pg_database WHERE NOT datistemplate AND datallowconn ORDER BY datname"
        }
        DriverKind::Mysql | DriverKind::MariaDb => "SHOW DATABASES",
        DriverKind::Redis => return Ok((0..16).map(|i| i.to_string()).collect()),
        _ => return Err("This is a file-based database: pick or type a file instead.".into()),
    };
    let res = run_admin(cfg, sql).await?;
    Ok(res
        .rows
        .iter()
        .filter_map(|r| r.first().and_then(|v| v.as_str()).map(str::to_string))
        .filter(|n| !SYSTEM_DATABASES.contains(&n.as_str()))
        .collect())
}

/// Creates a database on the server a connection points at. The name is normalised to
/// lowercase snake_case (letters, digits, `_`), so it never needs quoting tricks.
async fn create_database_for(cfg: &ConnectionConfig, name: &str) -> Result<String, String> {
    if cfg.read_only {
        return Err("This connection is read-only.".into());
    }
    if name.trim().is_empty() {
        return Err("Database name is required".into());
    }
    let db = svc::sanitize_db_name(name);
    let sql = match cfg.driver {
        DriverKind::Postgres => format!("CREATE DATABASE \"{db}\""),
        DriverKind::Mysql | DriverKind::MariaDb => format!("CREATE DATABASE `{db}`"),
        DriverKind::Redis => return Err("Redis has numbered databases (0–15); there is nothing to create.".into()),
        _ => return Err("For SQLite / DuckDB, type a new file name — it is created on first connect.".into()),
    };
    match run_admin(cfg, &sql).await {
        Ok(_) => Ok(db),
        Err(e) if e.contains("already exists") || e.contains("database exists") => {
            Err(format!("A database named \"{db}\" already exists"))
        }
        Err(e) => Err(e),
    }
}

/// Databases on the server of a connection being edited (form values, not yet saved).
#[tauri::command]
pub async fn server_list_databases(config: ConnectionConfig) -> Result<Vec<String>, String> {
    list_databases_for(&config).await
}

#[tauri::command]
pub async fn server_create_database(config: ConnectionConfig, name: String) -> Result<String, String> {
    create_database_for(&config, &name).await
}

/// Same, for a saved connection: credentials stay in Rust (read from the vault).
#[tauri::command]
pub async fn saved_list_databases(
    state: tauri::State<'_, AppState>,
    connection_id: String,
) -> Result<Vec<String>, String> {
    let cfg = resolve_connection(&state, &connection_id)?;
    list_databases_for(&cfg).await
}

#[tauri::command]
pub async fn saved_create_database(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    name: String,
) -> Result<String, String> {
    let cfg = resolve_connection(&state, &connection_id)?;
    create_database_for(&cfg, &name).await
}

/// User databases on a running service.
#[tauri::command]
pub async fn svc_list_databases(service_id: String) -> Result<Vec<String>, String> {
    let s = running_service(service_id).await?;
    list_databases_for(&service_connection(&s, None)).await
}

/// Creates a database on a running service.
#[tauri::command]
pub async fn svc_create_database(service_id: String, name: String) -> Result<String, String> {
    let s = running_service(service_id).await?;
    if s.engine == EngineType::Redis {
        return Err("Redis has numbered databases (0–15); there is nothing to create.".into());
    }
    create_database_for(&service_connection(&s, None), &name).await
}
#[tauri::command]
pub async fn svc_probe(service_id: String) -> Result<ProbeResult, String> {
    blocking(move || {
        let s = svc::get(&service_id).map_err(|e| e.to_string())?;
        svc::probe_detail(&s, std::time::Duration::from_millis(800)).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn svc_test_connection(service_id: String) -> Result<bool, String> {
    Ok(svc_probe(service_id).await?.reachable)
}

/// Start auto-start services in the background once the app is up.
pub fn spawn_service_autostart(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut cb = |id: &str, p: ServiceProgress| emit_progress(&app, id, p);
        svc::autostart(&mut cb);
        let _ = app.emit("svc-changed", ());
    });
}
