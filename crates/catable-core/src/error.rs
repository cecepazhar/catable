use thiserror::Error;

#[derive(Debug, Error)]
pub enum CataleError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("Database vault error: {0}")]
    Vault(String),

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
}
