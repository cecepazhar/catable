use crate::error::CatableError;
use rusqlite::Connection;

pub fn open() -> Result<Connection, CatableError> {
    let dir = crate::paths::app_data_dir()?;
    let path = dir.join("vault.db");
    let conn = Connection::open(&path).map_err(|e| CatableError::Vault(e.to_string()))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| CatableError::Vault(e.to_string()))?;
    Ok(conn)
}
