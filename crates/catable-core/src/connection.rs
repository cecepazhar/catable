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
}

impl ConnectionConfig {
    /// Returns true when the connection targets a production-grade
    /// environment tag, which the guardrail engine uses to tighten
    /// dangerous-query interception.
    pub fn is_production(&self) -> bool {
        matches!(self.environment, Environment::Production)
    }
}
