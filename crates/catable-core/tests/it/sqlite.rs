//! Throwaway SQLite database built from dev/seed/sqlite.sql.

use std::path::{Path, PathBuf};

use catable_core::{ConnectionConfig, DriverKind, Environment};

use crate::env::Target;

pub fn seeded_database() -> Target<ConnectionConfig> {
    let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dev/seed/sqlite.sql");
    let sql = std::fs::read_to_string(&seed)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", seed.display()));

    let path = std::env::temp_dir().join(format!("catable-it-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let db = rusqlite::Connection::open(&path)
        .unwrap_or_else(|e| panic!("cannot create {}: {e}", path.display()));
    db.execute_batch(&sql)
        .unwrap_or_else(|e| panic!("seeding {} failed: {e}", seed.display()));

    Ok(ConnectionConfig {
        connect_timeout_secs: None,
        id: "it-sqlite".into(),
        name: "IT Sqlite".into(),
        driver: DriverKind::Sqlite,
        environment: Environment::Development,
        host: String::new(),
        port: 0,
        database: path.to_string_lossy().into_owned(),
        user: String::new(),
        password: None,
        read_only: false,
        ssh_tunnel: None,
        group_name: None,
        workspace_id: None,
    })
}

pub fn remove_database(target: &Target<ConnectionConfig>) {
    if let Ok(conn) = target {
        let _ = std::fs::remove_file(PathBuf::from(&conn.database));
    }
}
