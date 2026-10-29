use crate::connection::{ConnectionConfig, DriverKind, Environment};
use crate::error::CatableError;
use crate::paths::app_data_dir;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use zeroize::Zeroize;

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    pub id: String,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInput {
    pub id: Option<String>,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalizedGroupRecord {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalizedGroupInput {
    pub id: Option<String>,
    pub workspace_id: String,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultBackupPayload {
    pub version: String,
    pub timestamp: i64,
    pub connections: Vec<ConnectionConfig>,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceRecord>,
    #[serde(default)]
    pub groups: Vec<PersonalizedGroupRecord>,
}

/// Global vault-unlock flag: true once a correct master password has been validated this session.
/// The Pro module reads this to decide whether to persist a sign-in immediately or hold it pending.
static UNLOCKED: AtomicBool = AtomicBool::new(false);

pub fn is_unlocked() -> Result<bool, CatableError> {
    Ok(UNLOCKED.load(Ordering::Relaxed))
}

pub fn mark_unlocked() {
    UNLOCKED.store(true, Ordering::Relaxed);
}

pub fn mark_locked() {
    UNLOCKED.store(false, Ordering::Relaxed);
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
                read_only   INTEGER NOT NULL DEFAULT 0,
                encrypted_password TEXT,
                ssh_tunnel  TEXT,
                group_name  TEXT,
                workspace_id TEXT DEFAULT 'default'
            );

            CREATE TABLE IF NOT EXISTS workspaces (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL UNIQUE,
                color       TEXT NOT NULL,
                description TEXT,
                created_at  INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS personalized_groups (
                id           TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                name         TEXT NOT NULL,
                color        TEXT NOT NULL,
                description  TEXT,
                created_at   INTEGER NOT NULL,
                UNIQUE(workspace_id, name)
            );
            "#,
        )
        .map_err(|e| CatableError::Vault(e.to_string()))?;

        // Ensure columns exist (migration)
        let column_exists = |col_name: &str| -> Result<bool, CatableError> {
            let mut stmt = conn
                .prepare("SELECT 1 FROM pragma_table_info('connections') WHERE name = ?1")
                .map_err(|e| CatableError::Vault(e.to_string()))?;
            stmt.exists(params![col_name])
                .map_err(|e| CatableError::Vault(e.to_string()))
        };

        if !column_exists("encrypted_password")? {
            conn.execute_batch("ALTER TABLE connections ADD COLUMN encrypted_password TEXT")
                .map_err(|e| CatableError::Vault(e.to_string()))?;
        }

        if !column_exists("ssh_tunnel")? {
            conn.execute_batch("ALTER TABLE connections ADD COLUMN ssh_tunnel TEXT")
                .map_err(|e| CatableError::Vault(e.to_string()))?;
        }

        if !column_exists("group_name")? {
            conn.execute_batch("ALTER TABLE connections ADD COLUMN group_name TEXT")
                .map_err(|e| CatableError::Vault(e.to_string()))?;
        }

        if !column_exists("workspace_id")? {
            conn.execute_batch("ALTER TABLE connections ADD COLUMN workspace_id TEXT DEFAULT 'default'")
                .map_err(|e| CatableError::Vault(e.to_string()))?;
        }

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

        let encrypted_password = cfg
            .password
            .as_ref()
            .filter(|p| !p.is_empty())
            .map(|p| encrypt_credential(p))
            .transpose()
            .map_err(|e| CatableError::Vault(format!("Failed to encrypt password: {}", e)))?;

        let encrypted_password_param: Option<&str> = encrypted_password.as_deref();

        let encrypted_ssh_tunnel = if let Some(ref ssh) = cfg.ssh_tunnel {
            let mut ssh_to_store = ssh.clone();
            if let Some(ref p) = ssh.password {
                if !p.is_empty() {
                    ssh_to_store.password = Some(encrypt_credential(p)?);
                }
            }
            if let Some(ref p) = ssh.key_passphrase {
                if !p.is_empty() {
                    ssh_to_store.key_passphrase = Some(encrypt_credential(p)?);
                }
            }
            Some(serde_json::to_string(&ssh_to_store).map_err(|e| CatableError::Serialization(e.to_string()))?)
        } else {
            None
        };
        let encrypted_ssh_tunnel_param: Option<&str> = encrypted_ssh_tunnel.as_deref();

        conn.execute(
            r#"
            INSERT INTO connections (id, name, driver, environment, host, port, database, user, read_only, encrypted_password, ssh_tunnel, group_name, workspace_id)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                driver = excluded.driver,
                environment = excluded.environment,
                host = excluded.host,
                port = excluded.port,
                database = excluded.database,
                user = excluded.user,
                read_only = excluded.read_only,
                encrypted_password = COALESCE(excluded.encrypted_password, connections.encrypted_password),
                ssh_tunnel = excluded.ssh_tunnel,
                group_name = excluded.group_name,
                workspace_id = excluded.workspace_id;
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
                if cfg.read_only { 1 } else { 0 },
                encrypted_password_param,
                encrypted_ssh_tunnel_param,
                cfg.group_name.as_deref(),
                cfg.workspace_id.as_deref().unwrap_or("default")
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
            .prepare("SELECT id, name, driver, environment, host, port, database, user, read_only, encrypted_password, ssh_tunnel, group_name, workspace_id FROM connections ORDER BY name ASC")
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        let rows = stmt
            .query_map([], |r| {
                let driver_s: String = r.get(2)?;
                let env_s: String = r.get(3)?;
                let read_only_int: i32 = r.get(8)?;
                let enc_pw: Option<String> = r.get(9)?;
                let enc_ssh: Option<String> = r.get(10)?;
                let group_name: Option<String> = r.get(11)?;
                let workspace_id: Option<String> = r.get(12).unwrap_or(Some("default".to_string()));

                let driver: DriverKind = serde_json::from_str(&format!("\"{}\"", driver_s))
                    .unwrap_or(DriverKind::Sqlite);
                let environment: Environment = serde_json::from_str(&format!("\"{}\"", env_s))
                    .unwrap_or(Environment::Development);

                // Decrypt password if present
                let password = match enc_pw.as_deref() {
                    Some(enc) if !enc.is_empty() => match decrypt_credential(enc) {
                        Ok(zeroizing) => Some((*zeroizing).clone()),
                        Err(e) => {
                            return Err(rusqlite::Error::FromSqlConversionFailure(
                                9,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            ));
                        }
                    },
                    _ => None,
                };

                // Decrypt ssh_tunnel if present
                let ssh_tunnel = match enc_ssh.as_deref() {
                    Some(s) if !s.trim().is_empty() => {
                        if let Ok(mut tunnel_cfg) = serde_json::from_str::<crate::connection::SshTunnelConfig>(s) {
                            if let Some(ref enc) = tunnel_cfg.password {
                                if let Ok(dec) = decrypt_credential(enc) {
                                    tunnel_cfg.password = Some((*dec).clone());
                                }
                            }
                            if let Some(ref enc) = tunnel_cfg.key_passphrase {
                                if let Ok(dec) = decrypt_credential(enc) {
                                    tunnel_cfg.key_passphrase = Some((*dec).clone());
                                }
                            }
                            Some(tunnel_cfg)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };

                Ok(ConnectionConfig {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    driver,
                    environment,
                    host: r.get(4)?,
                    port: r.get(5)?,
                    database: r.get(6)?,
                    user: r.get(7)?,
                    password,
                    read_only: read_only_int != 0,
                    ssh_tunnel,
                    group_name,
                    workspace_id,
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

    pub fn list_workspaces(&self) -> Result<Vec<WorkspaceRecord>, CatableError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT id, name, color, description, created_at FROM workspaces ORDER BY created_at ASC")
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(WorkspaceRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    description: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        let mut workspaces = Vec::new();
        for row in rows {
            workspaces.push(row.map_err(|e| CatableError::Vault(e.to_string()))?);
        }
        Ok(workspaces)
    }

    pub fn save_workspace(&self, input: &WorkspaceInput) -> Result<WorkspaceRecord, CatableError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        let now = chrono::Utc::now().timestamp();
        let id = input
            .id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let old_name: Option<String> = conn
            .query_row(
                "SELECT name FROM workspaces WHERE id = ?1",
                params![&id],
                |row| row.get(0),
            )
            .ok();

        conn.execute(
            r#"
            INSERT INTO workspaces (id, name, color, description, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                color = excluded.color,
                description = excluded.description
            "#,
            params![&id, input.name.trim(), &input.color, &input.description, now],
        )
        .map_err(|e| CatableError::Vault(e.to_string()))?;

        if let Some(old) = old_name {
            if old != input.name.trim() {
                let _ = conn.execute(
                    "UPDATE connections SET group_name = ?1 WHERE group_name = ?2",
                    params![input.name.trim(), old],
                );
            }
        }

        Ok(WorkspaceRecord {
            id,
            name: input.name.trim().to_string(),
            color: input.color.clone(),
            description: input.description.clone(),
            created_at: now,
        })
    }

    pub fn delete_workspace(&self, id: &str) -> Result<(), CatableError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        let name: Option<String> = conn
            .query_row(
                "SELECT name FROM workspaces WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .ok();

        conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id])
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        if let Some(wname) = name {
            let _ = conn.execute(
                "UPDATE connections SET group_name = NULL WHERE group_name = ?1",
                params![wname],
            );
        }

        Ok(())
    }

    pub fn list_groups(&self, workspace_id: Option<&str>) -> Result<Vec<PersonalizedGroupRecord>, CatableError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        let mut groups = Vec::new();
        if let Some(ws_id) = workspace_id {
            let mut stmt = conn
                .prepare("SELECT id, workspace_id, name, color, description, created_at FROM personalized_groups WHERE workspace_id = ?1 ORDER BY created_at ASC")
                .map_err(|e| CatableError::Vault(e.to_string()))?;
            let rows = stmt
                .query_map(params![ws_id], |row| {
                    Ok(PersonalizedGroupRecord {
                        id: row.get(0)?,
                        workspace_id: row.get(1)?,
                        name: row.get(2)?,
                        color: row.get(3)?,
                        description: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                })
                .map_err(|e| CatableError::Vault(e.to_string()))?;
            for row in rows {
                groups.push(row.map_err(|e| CatableError::Vault(e.to_string()))?);
            }
        } else {
            let mut stmt = conn
                .prepare("SELECT id, workspace_id, name, color, description, created_at FROM personalized_groups ORDER BY created_at ASC")
                .map_err(|e| CatableError::Vault(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(PersonalizedGroupRecord {
                        id: row.get(0)?,
                        workspace_id: row.get(1)?,
                        name: row.get(2)?,
                        color: row.get(3)?,
                        description: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                })
                .map_err(|e| CatableError::Vault(e.to_string()))?;
            for row in rows {
                groups.push(row.map_err(|e| CatableError::Vault(e.to_string()))?);
            }
        }
        Ok(groups)
    }

    pub fn save_group(&self, input: &PersonalizedGroupInput) -> Result<PersonalizedGroupRecord, CatableError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        let now = chrono::Utc::now().timestamp();
        let id = input
            .id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let old_name: Option<String> = conn
            .query_row(
                "SELECT name FROM personalized_groups WHERE id = ?1",
                params![&id],
                |row| row.get(0),
            )
            .ok();

        conn.execute(
            r#"
            INSERT INTO personalized_groups (id, workspace_id, name, color, description, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                workspace_id = excluded.workspace_id,
                name = excluded.name,
                color = excluded.color,
                description = excluded.description
            "#,
            params![
                &id,
                input.workspace_id.trim(),
                input.name.trim(),
                &input.color,
                &input.description,
                now
            ],
        )
        .map_err(|e| CatableError::Vault(e.to_string()))?;

        if let Some(old) = old_name {
            if old != input.name.trim() {
                let _ = conn.execute(
                    "UPDATE connections SET group_name = ?1 WHERE group_name = ?2",
                    params![input.name.trim(), old],
                );
            }
        }

        Ok(PersonalizedGroupRecord {
            id,
            workspace_id: input.workspace_id.trim().to_string(),
            name: input.name.trim().to_string(),
            color: input.color.clone(),
            description: input.description.clone(),
            created_at: now,
        })
    }

    pub fn delete_group(&self, id: &str) -> Result<(), CatableError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        if let Ok(name) = conn.query_row::<String, _, _>(
            "SELECT name FROM personalized_groups WHERE id = ?1",
            params![id],
            |row| row.get(0),
        ) {
            let _ = conn.execute(
                "UPDATE connections SET group_name = NULL WHERE group_name = ?1",
                params![&name],
            );
        }

        conn.execute("DELETE FROM personalized_groups WHERE id = ?1", params![id])
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        Ok(())
    }

    pub fn export_encrypted_backup(&self, passphrase: &str) -> Result<Vec<u8>, CatableError> {
        if passphrase.len() < MIN_PASSWORD_LEN {
            return Err(CatableError::Vault(format!(
                "Backup passphrase must be at least {} characters",
                MIN_PASSWORD_LEN
            )));
        }

        let connections = self.list_connections()?;
        let workspaces = self.list_workspaces()?;
        let groups = self.list_groups(None)?;
        let now = chrono::Utc::now().timestamp();

        let payload = VaultBackupPayload {
            version: "1.0.1".to_string(),
            timestamp: now,
            connections,
            workspaces,
            groups,
        };

        let json_bytes = serde_json::to_vec(&payload)
            .map_err(|e| CatableError::Serialization(e.to_string()))?;

        // Derive 32-byte key using Argon2id
        let mut derived_key = [0u8; 32];
        let params = argon2::Params::new(64 * 1024, 3, 4, Some(32))
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        let argon2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
        let salt = b"catable.vault.backup.salt.2026";
        argon2
            .hash_password_into(passphrase.as_bytes(), salt, &mut derived_key)
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        // Encrypt with AES-256-GCM
        let cipher = Aes256Gcm::new_from_slice(&derived_key)
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes.copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..12]);
        let nonce = aes_gcm::aead::Nonce::<Aes256Gcm>::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, json_bytes.as_slice())
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    pub fn import_encrypted_backup(&self, data: &[u8], passphrase: &str) -> Result<usize, CatableError> {
        if data.len() < 12 {
            return Err(CatableError::Vault("Encrypted backup payload is too short".into()));
        }

        let mut derived_key = [0u8; 32];
        let params = argon2::Params::new(64 * 1024, 3, 4, Some(32))
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        let argon2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
        let salt = b"catable.vault.backup.salt.2026";
        argon2
            .hash_password_into(passphrase.as_bytes(), salt, &mut derived_key)
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        let cipher = Aes256Gcm::new_from_slice(&derived_key)
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        let nonce_bytes = &data[..12];
        let ciphertext = &data[12..];
        let nonce = aes_gcm::aead::Nonce::<Aes256Gcm>::from_slice(nonce_bytes);

        let decrypted = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| CatableError::Vault(format!("Decryption failed: check passphrase or file integrity ({e})")))?;

        let payload: VaultBackupPayload = serde_json::from_slice(&decrypted)
            .map_err(|e| CatableError::Serialization(format!("Corrupt backup file: {e}")))?;

        self.apply_backup_payload(payload)
    }

    pub fn export_json_backup(&self) -> Result<String, CatableError> {
        let connections = self.list_connections()?;
        let workspaces = self.list_workspaces()?;
        let groups = self.list_groups(None)?;
        let now = chrono::Utc::now().timestamp();

        let payload = VaultBackupPayload {
            version: "1.0.1".to_string(),
            timestamp: now,
            connections,
            workspaces,
            groups,
        };

        serde_json::to_string_pretty(&payload)
            .map_err(|e| CatableError::Serialization(e.to_string()))
    }

    pub fn import_json_backup(&self, json_str: &str) -> Result<usize, CatableError> {
        let payload: VaultBackupPayload = serde_json::from_str(json_str)
            .map_err(|e| CatableError::Serialization(format!("Invalid JSON backup format: {e}")))?;

        self.apply_backup_payload(payload)
    }

    pub fn apply_backup_payload(&self, payload: VaultBackupPayload) -> Result<usize, CatableError> {
        let mut count = 0;
        for ws in payload.workspaces {
            let _ = self.save_workspace(&WorkspaceInput {
                id: Some(ws.id),
                name: ws.name,
                color: ws.color,
                description: ws.description,
            });
            count += 1;
        }

        for grp in payload.groups {
            let _ = self.save_group(&PersonalizedGroupInput {
                id: Some(grp.id),
                workspace_id: grp.workspace_id,
                name: grp.name,
                color: grp.color,
                description: grp.description,
            });
            count += 1;
        }

        for conn in payload.connections {
            let _ = self.save_connection(&conn);
            count += 1;
        }

        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// AES-256-GCM credential encryption (encrypted credential blobs)
// ---------------------------------------------------------------------------

/// Returns the AES-256-GCM key (32 bytes) used to encrypt credentials.
/// The key is generated on first use and persisted so credentials can be
/// decrypted on subsequent runs.
pub fn get_vault_encryption_key() -> Result<[u8; 32], CatableError> {
    let dir = app_data_dir()?;
    let key_path = dir.join("vault_key.bin");

    if let Ok(bytes) = std::fs::read(&key_path) {
        if let Ok(key) = <[u8; 32]>::try_from(bytes.as_slice()) {
            return Ok(key);
        }
    }

    // Generate a random 32-byte key and store it
    let key = {
        let mut buf: [u8; 32] = [0u8; 32];
        use std::io::Read;
        let mut f = std::fs::File::open("/dev/urandom").ok();
        if let Some(f) = f.as_mut() {
            f.read_exact(&mut buf).ok();
        } else {
            buf[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
            buf[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        }
        buf
    };
    std::fs::write(&key_path, key).map_err(|e| CatableError::Io(e.to_string()))?;
    // Best-effort to tighten permissions on the key file
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

/// Encrypts a credential string using AES-256-GCM with a random 12-byte nonce.
/// Returns the base64-encoded ciphertext (nonce || cipher).
fn encrypt_credential(password: &str) -> Result<String, CatableError> {
    let key = get_vault_encryption_key()?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| CatableError::Vault(e.to_string()))?;

    // Generate a random 12-byte nonce
    let mut nonce_bytes = [0u8; 12];
    nonce_bytes.copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..12]);
    let nonce = aes_gcm::aead::Nonce::<Aes256Gcm>::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, password.as_bytes())
        .map_err(|e| CatableError::Vault(e.to_string()))?;

    // Prepend the nonce to the ciphertext, encode the whole thing as base64
    let mut blob = Vec::with_capacity(nonce_bytes.len() + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &blob))
}

/// Decrypts a credential string (base64-encoded AES-256-GCM blob) and
/// returns a `zeroize::Zeroizing<String>` so plaintext is wiped on drop.
fn decrypt_credential(encrypted: &str) -> Result<zeroize::Zeroizing<String>, CatableError> {
    let key = get_vault_encryption_key()?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| CatableError::Vault(e.to_string()))?;

    let decoded =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encrypted)
            .map_err(|e| CatableError::Vault(format!("Base64 decode failed: {}", e)))?;

    if decoded.len() < 12 {
        return Err(CatableError::Vault("Encrypted blob too short".into()));
    }

    let nonce_bytes = &decoded[..12];
    let ciphertext = &decoded[12..];
    let nonce = aes_gcm::aead::Nonce::<Aes256Gcm>::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| CatableError::Vault(format!("Decryption failed: {}", e)))?;

    let password = String::from_utf8(plaintext)
        .map_err(|e| CatableError::Vault(format!("Invalid UTF-8: {}", e)))?;

    Ok(zeroize::Zeroizing::new(password))
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
    let mut actual = derive_hash(password, &salt)?;

    if !canary.exists() {
        std::fs::write(&canary, &actual).map_err(|e| CatableError::Io(e.to_string()))?;
        actual.zeroize();
        mark_unlocked();
        return Ok(true);
    }

    let stored = std::fs::read(&canary).map_err(|e| CatableError::Io(e.to_string()))?;
    let expected: [u8; 32] = stored
        .as_slice()
        .try_into()
        .map_err(|_| CatableError::InvalidPassword)?;

    let valid = constant_time_eq(&actual, &expected);
    actual.zeroize();
    if valid {
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

    // Remove the encryption key file so credentials can no longer be decrypted
    let key_path = app_data_dir()?.join("vault_key.bin");
    if key_path.exists() {
        std::fs::remove_file(&key_path).map_err(|e| CatableError::Io(e.to_string()))?;
    }

    // Remove the encrypted vault database itself
    let db_path = app_data_dir()?.join("vault.db");
    if db_path.exists() {
        let _ = std::fs::remove_file(&db_path);
    }
    // Also remove WAL/SHM sidecar files
    let db_shm = app_data_dir()?.join("vault.db-shm");
    let db_wal = app_data_dir()?.join("vault.db-wal");
    let _ = std::fs::remove_file(&db_shm);
    let _ = std::fs::remove_file(&db_wal);

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
    let mut old_hash = derive_hash(old_password, &salt)?;
    let valid = constant_time_eq(&old_hash, &expected);
    old_hash.zeroize();
    if !valid {
        return Err(CatableError::InvalidPassword);
    }

    let mut new_hash = derive_hash(new_password, &salt)?;
    std::fs::write(&canary, &new_hash).map_err(|e| CatableError::Io(e.to_string()))?;
    new_hash.zeroize();
    Ok(())
}

/// Constant-time byte array comparison to avoid timing attacks.
fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let _guard = TEST_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!(
            "catable-test-enc-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        unsafe { std::env::set_var("CATABLE_DATA_DIR", &tmp) };

        let pw = "test_password_123".to_string();
        let encrypted = encrypt_credential(&pw).unwrap();
        let decrypted: zeroize::Zeroizing<String> = decrypt_credential(&encrypted).unwrap();
        assert_eq!(*decrypted, pw);
    }

    #[test]
    fn test_vault_stores_encrypted_password() {
        let _guard = TEST_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!(
            "catable-test-vault-enc-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        unsafe { std::env::set_var("CATABLE_DATA_DIR", &tmp) };

        let vault = Vault::open().unwrap();
        let cfg = ConnectionConfig {
            id: "conn-enc".into(),
            name: "Encrypted Pg".into(),
            driver: DriverKind::Postgres,
            environment: Environment::Development,
            host: "127.0.0.1".into(),
            port: 5432,
            database: "testdb".into(),
            user: "testuser".into(),
            password: Some("secretpass".into()),
            read_only: false,
            ssh_tunnel: None,
            group_name: None,
        };

        vault.save_connection(&cfg).unwrap();
        let list = vault.list_connections().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].password.as_deref(), Some("secretpass"));
    }

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
            ssh_tunnel: None,
            group_name: None,
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
        let tmp = std::env::temp_dir().join(format!(
            "catable-test-vault-{}",
            uuid::Uuid::now_v7()
        ));
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

    #[test]
    fn test_workspaces_and_backup_roundtrip() {
        let _guard = TEST_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("catable-test-ws-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&tmp).unwrap();
        unsafe { std::env::set_var("CATABLE_DATA_DIR", &tmp) };

        let vault = Vault::open().unwrap();

        // 1. Create workspace
        let ws = vault
            .save_workspace(&WorkspaceInput {
                id: None,
                name: "Production Fleet".into(),
                color: "#ef4444".into(),
                description: Some("Prod databases".into()),
            })
            .unwrap();
        assert_eq!(ws.name, "Production Fleet");

        let workspaces = vault.list_workspaces().unwrap();
        assert_eq!(workspaces.len(), 1);

        // 2. Create connection in workspace
        let cfg = ConnectionConfig {
            id: "db-1".into(),
            name: "Main DB".into(),
            driver: DriverKind::Postgres,
            environment: Environment::Production,
            host: "db.internal".into(),
            port: 5432,
            database: "primary".into(),
            user: "postgres".into(),
            password: Some("secret123".into()),
            read_only: false,
            ssh_tunnel: None,
            group_name: Some("Production Fleet".into()),
        };
        vault.save_connection(&cfg).unwrap();

        // 3. Encrypted backup roundtrip
        let backup_bytes = vault.export_encrypted_backup("backup-passphrase-888").unwrap();
        assert!(backup_bytes.len() > 12);

        // 4. JSON backup roundtrip
        let json_str = vault.export_json_backup().unwrap();
        assert!(json_str.contains("Production Fleet"));
        assert!(json_str.contains("Main DB"));

        // 5. Delete connection & workspace
        vault.delete_connection("db-1").unwrap();
        vault.delete_workspace(&ws.id).unwrap();
        assert_eq!(vault.list_connections().unwrap().len(), 0);
        assert_eq!(vault.list_workspaces().unwrap().len(), 0);

        // 6. Restore from encrypted backup
        let count = vault.import_encrypted_backup(&backup_bytes, "backup-passphrase-888").unwrap();
        assert_eq!(count, 2); // 1 ws + 1 conn

        let restored_conns = vault.list_connections().unwrap();
        assert_eq!(restored_conns.len(), 1);
        assert_eq!(restored_conns[0].name, "Main DB");
        assert_eq!(restored_conns[0].password.as_deref(), Some("secret123"));

        let restored_ws = vault.list_workspaces().unwrap();
        assert_eq!(restored_ws.len(), 1);
        assert_eq!(restored_ws[0].name, "Production Fleet");
    }
}
