mod commands;

pub use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::list_connections,
            commands::save_connection,
            commands::delete_connection,
            commands::list_workspaces,
            commands::save_workspace,
            commands::delete_workspace,
            commands::list_groups,
            commands::save_group,
            commands::delete_group,
            commands::export_encrypted_backup,
            commands::import_encrypted_backup,
            commands::export_json_backup,
            commands::import_json_backup,
            commands::evaluate_query_safety,
            commands::test_database_connection,
            commands::test_ssh_tunnel,
            commands::session_open,
            commands::session_close,
            commands::session_execute,
            commands::session_list_tables,
            commands::session_ping,
            commands::session_browse,
            commands::session_build_select,
            commands::session_describe_table,
            commands::session_preview_changes,
            commands::session_apply_changes,
            commands::get_app_version,
            commands::is_vault_unlocked,
            commands::is_vault_initialized,
            commands::validate_vault_password,
            commands::reset_vault,
            commands::lock_vault,
            commands::change_master_password,
            commands::window_minimize,
            commands::window_maximize,
            commands::window_close,
            commands::window_start_dragging,
            // Pro commands
            commands::pro_status,
            commands::pro_server_available,
            commands::pro_register,
            commands::pro_resend_verification,
            commands::pro_forgot_password,
            commands::pro_login,
            commands::pro_commit_pending,
            commands::pro_sync,
            commands::pro_start_trial,
            commands::pro_account,
            commands::pro_revoke_device,
            commands::pro_logout,
            commands::pro_activate_license,
            commands::pro_team,
            commands::pro_team_invite,
            commands::pro_team_cancel_invite,
            commands::pro_team_remove_member,
            commands::pro_team_accept,
            commands::pro_team_decline,
            commands::pro_team_leave,
            // Teams aliases
            commands::teams_view,
            commands::teams_invite,
            commands::teams_cancel_invite,
            commands::teams_remove_member,
            commands::teams_accept,
            commands::teams_decline,
            commands::teams_leave,
            // Billing commands
            commands::billing_get_effective_state,
            commands::billing_request_otp,
            commands::billing_verify_otp,
            commands::billing_activate_license,
            commands::billing_create_checkout,
            commands::billing_poll_checkout,
            commands::billing_sign_out,
            // Pro features
            commands::pro_sync_vault,
            commands::pro_ssh_tunnel,
            commands::evaluate_query_blast_radius,
            // AI DBA Copilot
            commands::ai_chat,
            commands::ai_explain,
            // DB Service Engine Manager (CAT-SVC)
            commands::svc_list,
            commands::svc_get,
            commands::svc_create,
            commands::svc_stop,
            commands::svc_remove,
            commands::svc_open_in_studio
        ]);

    builder
        .run(tauri::generate_context!())
        .expect("error while running CATable application");
}
