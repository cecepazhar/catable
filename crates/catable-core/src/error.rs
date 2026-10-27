use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum ProError {
    #[error("{0}")]
    Generic(String),
}

impl ProError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Generic(_) => "CAT-PRO-000",
        }
    }
}

#[derive(Debug, Clone, Error)]
pub enum CatermError {
    #[error("vault: {0}")]
    Vault(String),

    #[error("database: {0}")]
    Db(String),

    #[error("tunnel: {0}")]
    Tunnel(String),

    #[error("sync: {0}")]
    Sync(String),

    #[error("pro: {0}")]
    Pro(String),

    #[error("pro license required: {0}")]
    ProRequired(String),

    #[error("billing: {0}")]
    Billing(String),

    #[error("internal: {0}")]
    Internal(String),
}

impl CatermError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Vault(_) => "CAT-VAULT-000",
            Self::Db(_) => "CAT-DB-000",
            Self::Tunnel(_) => "CAT-TUNNEL-000",
            Self::Sync(_) => "CAT-SYNC-000",
            Self::Pro(_) => "CAT-PRO-000",
            Self::ProRequired(_) => "CAT-PRO-001",
            Self::Billing(_) => "CAT-BILLING-000",
            Self::Internal(_) => "CAT-CORE-500",
        }
    }

    pub fn domain(&self) -> &'static str {
        match self {
            Self::Vault(_) => "VAULT",
            Self::Db(_) => "DB",
            Self::Tunnel(_) => "TUNNEL",
            Self::Sync(_) => "SYNC",
            Self::Pro(_) | Self::ProRequired(_) => "PRO",
            Self::Billing(_) => "BILLING",
            Self::Internal(_) => "CORE",
        }
    }
}

impl Serialize for CatermError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("CatermError", 3)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.serialize_field("domain", self.domain())?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for CatermError {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct RawError {
            code: Option<String>,
            message: Option<String>,
        }
        let raw = RawError::deserialize(deserializer)?;
        let msg = raw.message.unwrap_or_else(|| "unknown error".into());
        match raw.code.as_deref() {
            Some("CAT-PRO-000") => Ok(CatermError::Pro(msg)),
            Some("CAT-PRO-001") => Ok(CatermError::ProRequired(msg)),
            Some("CAT-VAULT-000") => Ok(CatermError::Vault(msg)),
            Some("CAT-DB-000") => Ok(CatermError::Db(msg)),
            Some("CAT-TUNNEL-000") => Ok(CatermError::Tunnel(msg)),
            Some("CAT-SYNC-000") => Ok(CatermError::Sync(msg)),
            Some("CAT-BILLING-000") => Ok(CatermError::Billing(msg)),
            _ => Ok(CatermError::Internal(msg)),
        }
    }
}

impl From<rusqlite::Error> for CatermError {
    fn from(e: rusqlite::Error) -> Self {
        CatermError::Db(e.to_string())
    }
}

impl From<CatableError> for CatermError {
    fn from(e: CatableError) -> Self {
        match e {
            CatableError::Vault(m) => CatermError::Vault(m),
            CatableError::Db(m) => CatermError::Db(m),
            CatableError::Pro(m) => CatermError::Pro(m),
            other => CatermError::Internal(other.to_string()),
        }
    }
}

#[derive(Debug, Error)]
pub enum CatableError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("Database vault error: {0}")]
    Vault(String),

    #[error("Database error: {0}")]
    Db(String),

    #[error("Pro error: {0}")]
    Pro(String),

    #[error("Invalid master password or corrupted vault")]
    InvalidPassword,

    #[error("Connection not found: {0}")]
    ConnectionNotFound(String),

    #[error("Guardrail violation: {0}")]
    GuardrailViolation(String),

    #[error("Driver error ({driver}): {message}")]
    Driver {
        driver: String,
        message: String,
    },

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Tunnel error: {0}")]
    Tunnel(String),
}
