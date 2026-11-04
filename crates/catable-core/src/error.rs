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


/// Coarse category of a failed connection attempt, so the UI can show a
/// specific hint instead of a raw driver string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectErrorKind {
    Refused,
    Timeout,
    AuthFailed,
    TlsFailed,
    TunnelFailed,
    DnsFailed,
    Other,
}

impl ConnectErrorKind {
    /// Classifies a driver/tunnel error message. Order matters: the tunnel
    /// prefix wins, then the most specific network/auth causes.
    pub fn classify(message: &str) -> Self {
        let m = message.to_lowercase();
        let has = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
        if has(&["ssh tunnel", "tunnel:", "ssh host", "ssh agent", "ssh handshake"]) {
            Self::TunnelFailed
        } else if has(&["timed out", "timeout", "os error 110", "os error 10060"]) {
            Self::Timeout
        } else if has(&["connection refused", "os error 111", "os error 10061", "actively refused"]) {
            Self::Refused
        } else if has(&[
            "failed to lookup",
            "name or service not known",
            "no such host",
            "nodename nor servname",
            "failed to resolve",
            "dns error",
        ]) {
            Self::DnsFailed
        } else if has(&[
            "access denied",
            "password authentication failed",
            "authentication failed",
            "28p01",
            "28000",
            "error 1045",
            "auth plugin",
        ]) {
            Self::AuthFailed
        } else if has(&["tls", "ssl", "certificate"]) {
            Self::TlsFailed
        } else {
            Self::Other
        }
    }

    /// Short actionable hint (English; the UI localizes by `kind`).
    pub fn hint(self) -> &'static str {
        match self {
            Self::Refused => "Nothing is listening on that host/port. Check the port and that the server is running.",
            Self::Timeout => "No response in time. Check firewall/port, VPN, or whether an SSH tunnel is required.",
            Self::AuthFailed => "The server rejected the credentials. Check user, password and database.",
            Self::TlsFailed => "TLS/SSL negotiation failed. Check the server's SSL settings.",
            Self::TunnelFailed => "The SSH tunnel could not be established. Test the SSH settings first.",
            Self::DnsFailed => "The host name could not be resolved. Check the host spelling and DNS.",
            Self::Other => "Connection failed. See the message for details.",
        }
    }
}

#[cfg(test)]
mod connect_error_kind_tests {
    use super::ConnectErrorKind as K;

    #[test]
    fn classifies_common_messages() {
        assert_eq!(K::classify("mysql connection timed out after 10s"), K::Timeout);
        assert_eq!(K::classify("error connecting: Connection refused (os error 111)"), K::Refused);
        assert_eq!(K::classify("db error: FATAL: password authentication failed for user \"x\""), K::AuthFailed);
        assert_eq!(K::classify("Access denied for user 'root'@'h' (using password: YES)"), K::AuthFailed);
        assert_eq!(K::classify("failed to lookup address information: Name or service not known"), K::DnsFailed);
        assert_eq!(K::classify("SSH Tunnel connection failed: x"), K::TunnelFailed);
        assert_eq!(K::classify("TLS handshake failed"), K::TlsFailed);
        assert_eq!(K::classify("something odd"), K::Other);
    }

    #[test]
    fn tunnel_prefix_beats_timeout() {
        assert_eq!(K::classify("SSH Tunnel connection failed: timed out"), K::TunnelFailed);
    }
}
