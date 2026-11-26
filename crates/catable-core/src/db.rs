use crate::error::CatableError;
use rusqlite::Connection;

pub fn open() -> Result<Connection, CatableError> {
    let dir = crate::paths::app_data_dir()?;
    crate::secure_store::open_vault_db(&dir)
}
