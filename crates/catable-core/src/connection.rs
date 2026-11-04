use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Development,
    Staging,
    Production,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverKind {
    Sqlite,
    Postgres,
    Mysql,
    MariaDb,
    DuckDb,
    Redis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshAuthType {
    Password,
    #[serde(rename = "key")]
    PrivateKey,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshTunnelConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth_type: SshAuthType,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub key_passphrase: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

/// Describes a saved database connection profile.
///
/// `password` is only held in this struct during in-memory use; it is
/// never serialized to the vault database — instead the vault stores a
/// pointer to an encrypted secret blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub driver: DriverKind,
    pub environment: Environment,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// `None` when the credential is stored encrypted in the vault.
    pub password: Option<String>,
    /// Read-only driver mode is enforced at the connection level, not just
    /// in the UI, so AI-generated queries cannot write to a read-only
    /// connection (PRD §3.5 "production-safe by default").
    pub read_only: bool,
    #[serde(default)]
    pub ssh_tunnel: Option<SshTunnelConfig>,
    #[serde(default)]
    pub group_name: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Connect timeout in seconds. `None` = [`DEFAULT_CONNECT_TIMEOUT_SECS`].
    #[serde(default)]
    pub connect_timeout_secs: Option<u64>,
}

/// Id of the built-in sandbox connection (a seeded local SQLite file).
pub const SANDBOX_CONNECTION_ID: &str = "conn-sandbox";

/// Default connect timeout when a connection does not set its own.
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

impl ConnectionConfig {
    /// Returns true when the connection targets a production-grade
    /// environment tag, which the guardrail engine uses to tighten
    /// dangerous-query interception.
    pub fn is_production(&self) -> bool {
        matches!(self.environment, Environment::Production)
    }

    /// The built-in sandbox connection (not stored in the vault).
    pub fn sandbox() -> Self {
        Self {
            id: SANDBOX_CONNECTION_ID.into(),
            name: "Local Sandbox DB".into(),
            driver: DriverKind::Sqlite,
            environment: Environment::Development,
            host: "localhost".into(),
            port: 0,
            database: "sandbox.db".into(),
            user: "admin".into(),
            password: None,
            read_only: false,
            ssh_tunnel: None,
            group_name: None,
            workspace_id: None,
            connect_timeout_secs: None,
        }
    }

    /// Effective connect timeout, clamped to 1..=120 seconds.
    pub fn connect_timeout(&self) -> std::time::Duration {
        let secs = self
            .connect_timeout_secs
            .unwrap_or(DEFAULT_CONNECT_TIMEOUT_SECS)
            .clamp(1, 120);
        std::time::Duration::from_secs(secs)
    }

    /// Returns the effective TCP port for this connection, resolving default
    /// ports when port is 0:
    /// - PostgreSQL: 5432
    /// - MySQL / MariaDB: 3306
    /// - Redis: 6379
    pub fn effective_port(&self) -> u16 {
        if self.port != 0 {
            return self.port;
        }
        match self.driver {
            DriverKind::Postgres => 5432,
            DriverKind::Mysql | DriverKind::MariaDb => 3306,
            DriverKind::Redis => 6379,
            _ => 0,
        }
    }

    /// Securely wipes the in-memory credential buffer, zeroizing the
    /// underlying plaintext bytes before clearing the field. Call this
    /// after the driver has finished using the password so no plaintext
    /// lingers on the heap.
    pub fn zeroize_password(&mut self) {
        use zeroize::Zeroize;
        if let Some(ref mut pw) = self.password {
            pw.zeroize();
        }
        self.password = None;
    }
}
