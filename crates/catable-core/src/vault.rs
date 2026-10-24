use crate::connection::{ConnectionConfig, DriverKind, Environment};
use crate::error::CatableError;
use crate::paths::app_data_dir;
use rusqlite::{params, Connection};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

/// Global vault-unlock flag: true once a correct master password has been validated this session.
/// The Pro module reads this to decide whether to persist a sign-in immediately or hold it pending.
static UNLOCKED: AtomicBool = AtomicBool::new(false);

pub fn is_unlocked() -> Result<bool, CatableError> {
    Ok(UNLOCKED.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn mark_unlocked() {
    UNLOCKED.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn mark_locked() {
    UNLOCKED.store(false, std::sync::atomic::Ordering::Relaxed);
}

pub struct Vault {
    conn: Mutex<Connection>,
}

impl Vault {
    pub fn open() -> Result<Self, CatableError> {
        let dir = app_data_dir()?;
        let path = dir.join("vault.db");
        let conn = Connection::open(&path).map_err(|e| CatableError::Vault(e.to_string()))?;

        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS connections (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                driver      TEXT NOT NULL,
                environment TEXT NOT NULL,
                host        TEXT NOT NULL,
                port        INTEGER NOT NULL,
                database    TEXT NOT NULL,
                user        TEXT NOT NULL,
                read_only   INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .map_err(|e| CatableError::Vault(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn save_connection(&self, cfg: &ConnectionConfig) -> Result<(), CatableError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        let driver_str = serde_json::to_string(&cfg.driver)
            .map_err(|e| CatableError::Serialization(e.to_string()))?;
        let env_str = serde_json::to_string(&cfg.environment)
            .map_err(|e| CatableError::Serialization(e.to_string()))?;

        conn.execute(
            r#"
            INSERT INTO connections (id, name, driver, environment, host, port, database, user, read_only)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                driver = excluded.driver,
                environment = excluded.environment,
                host = excluded.host,
                port = excluded.port,
                database = excluded.database,
                user = excluded.user,
                read_only = excluded.read_only;
            "#,
            params![
                cfg.id,
                cfg.name,
                driver_str.trim_matches('"'),
                env_str.trim_matches('"'),
                cfg.host,
                cfg.port,
                cfg.database,
                cfg.user,
                if cfg.read_only { 1 } else { 0 }
            ],
        )
        .map_err(|e| CatableError::Vault(e.to_string()))?;

        Ok(())
    }

    pub fn list_connections(&self) -> Result<Vec<ConnectionConfig>, CatableError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT id, name, driver, environment, host, port, database, user, read_only FROM connections ORDER BY name ASC")
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        let rows = stmt
            .query_map([], |r| {
                let driver_s: String = r.get(2)?;
                let env_s: String = r.get(3)?;
                let read_only_int: i32 = r.get(8)?;

                let driver: DriverKind = serde_json::from_str(&format!("\"{}\"", driver_s))
                    .unwrap_or(DriverKind::Sqlite);
                let environment: Environment = serde_json::from_str(&format!("\"{}\"", env_s))
                    .unwrap_or(Environment::Development);

                Ok(ConnectionConfig {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    driver,
                    environment,
                    host: r.get(4)?,
                    port: r.get(5)?,
                    database: r.get(6)?,
                    user: r.get(7)?,
                    password: None,
                    read_only: read_only_int != 0,
                })
            })
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| CatableError::Vault(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn delete_connection(&self, id: &str) -> Result<(), CatableError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        conn.execute("DELETE FROM connections WHERE id = ?1", params![id])
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        Ok(())
    }
}

pub const MIN_PASSWORD_LEN: usize = 8;
const CANARY_FILE: &str = "vault_canary.bin";
const SALT_FILE: &str = "vault_salt.bin";
const SALT_LEN: usize = 16;

fn canary_path() -> Result<std::path::PathBuf, CatableError> {
    let dir = app_data_dir()?;
    Ok(dir.join(CANARY_FILE))
}

fn salt_path() -> Result<std::path::PathBuf, CatableError> {
    let dir = app_data_dir()?;
    Ok(dir.join(SALT_FILE))
}

pub fn is_vault_initialized() -> Result<bool, CatableError> {
    Ok(canary_path()?.exists())
}

fn load_or_create_salt() -> Result<[u8; SALT_LEN], CatableError> {
    let path = salt_path()?;
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(salt) = <[u8; SALT_LEN]>::try_from(bytes.as_slice()) {
            return Ok(salt);
        }
    }

    let salt = *uuid::Uuid::new_v4().as_bytes();
    std::fs::write(&path, salt).map_err(|e| CatableError::Io(e.to_string()))?;
    Ok(salt)
}

fn derive_hash(password: &str, salt: &[u8]) -> Result<[u8; 32], CatableError> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(CatableError::InvalidPassword);
    }

    use argon2::{Argon2, Params, Version};

    let cost = Params::new(64 * 1024, 3, 4, Some(32))
        .map_err(|e| CatableError::Vault(e.to_string()))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, cost);

    let mut derived = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut derived)
        .map_err(|e| CatableError::Vault(e.to_string()))?;
    Ok(derived)
}

pub fn validate_vault_password(password: &str) -> Result<bool, CatableError> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(CatableError::InvalidPassword);
    }

    let canary = canary_path()?;
    let salt = load_or_create_salt()?;
    let actual = derive_hash(password, &salt)?;

    if !canary.exists() {
        std::fs::write(&canary, actual).map_err(|e| CatableError::Io(e.to_string()))?;
        mark_unlocked();
        return Ok(true);
    }

    let stored = std::fs::read(&canary).map_err(|e| CatableError::Io(e.to_string()))?;
    let expected: [u8; 32] = stored
        .as_slice()
        .try_into()
        .map_err(|_| CatableError::InvalidPassword)?;

    if actual == expected {
        mark_unlocked();
        Ok(true)
    } else {
        mark_locked();
        Err(CatableError::InvalidPassword)
    }
}

pub fn reset_vault() -> Result<(), CatableError> {
    mark_locked();
    let canary = canary_path()?;
    if canary.exists() {
        std::fs::remove_file(&canary).map_err(|e| CatableError::Io(e.to_string()))?;
    }

    let salt = salt_path()?;
    if salt.exists() {
        std::fs::remove_file(&salt).map_err(|e| CatableError::Io(e.to_string()))?;
    }

    Ok(())
}

pub fn change_master_password(old_password: &str, new_password: &str) -> Result<(), CatableError> {
    if new_password.len() < MIN_PASSWORD_LEN {
        return Err(CatableError::InvalidPassword);
    }

    let canary = canary_path()?;
    let salt = load_or_create_salt()?;

    if !canary.exists() {
        return Err(CatableError::InvalidPassword);
    }

    // verify old password
    let stored = std::fs::read(&canary).map_err(|e| CatableError::Io(e.to_string()))?;
    let expected: [u8; 32] = stored
        .as_slice()
        .try_into()
        .map_err(|_| CatableError::InvalidPassword)?;
    let old_hash = derive_hash(old_password, &salt)?;
    if old_hash != expected {
        return Err(CatableError::InvalidPassword);
    }

    let new_hash = derive_hash(new_password, &salt)?;
    std::fs::write(&canary, new_hash).map_err(|e| CatableError::Io(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    #[test]
    fn roundtrip_connection_persistence() {
        let _guard = TEST_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("catable-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&tmp).unwrap();
        unsafe { std::env::set_var("CATABLE_DATA_DIR", &tmp) };

        let vault = Vault::open().unwrap();
        let cfg = ConnectionConfig {
            id: "conn-1".into(),
            name: "Local Postgres".into(),
            driver: DriverKind::Postgres,
            environment: Environment::Development,
            host: "127.0.0.1".into(),
            port: 5432,
            database: "app".into(),
            user: "postgres".into(),
            password: None,
            read_only: false,
        };

        vault.save_connection(&cfg).unwrap();
        let list = vault.list_connections().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Local Postgres");

        vault.delete_connection("conn-1").unwrap();
        assert_eq!(vault.list_connections().unwrap().len(), 0);
    }

    #[test]
    fn vault_initialization_and_password_lifecycle() {
        let _guard = TEST_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("catable-test-vault-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&tmp).unwrap();
        unsafe { std::env::set_var("CATABLE_DATA_DIR", &tmp) };

        assert!(!is_vault_initialized().unwrap());

        // Setup password (first time initializes)
        assert!(validate_vault_password("masterpass123").unwrap());
        assert!(is_vault_initialized().unwrap());

        // Validate again with correct password
        assert!(validate_vault_password("masterpass123").unwrap());

        // Validate with wrong password
        assert!(validate_vault_password("wrongpassword").is_err());

        // Change master password
        assert!(change_master_password("masterpass123", "newmasterpass456").is_ok());
        assert!(validate_vault_password("newmasterpass456").unwrap());
        assert!(validate_vault_password("masterpass123").is_err());

        // Reset vault
        assert!(reset_vault().is_ok());
        assert!(!is_vault_initialized().unwrap());
    }
}
