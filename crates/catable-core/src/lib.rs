//! CATable core — encrypted connection vault, SQL guardrails & driver registry.
//!
//! Design notes (PRD §3.4 & §4):
//! - Credentials & SSH keys live in a local SQLCipher database; plaintext
//!   secrets are zeroized out of memory after use.
//! - Every query (user-authored, AI-generated, or inline edit) passes through
//!   the guardrail engine before reaching a live driver.
//! - The driver registry exposes a capability matrix so the UI can disable
//!   unsupported features per backend instead of failing at runtime.

mod connection;
mod driver;
mod error;
mod executor;
mod guard;
mod paths;
mod vault;
pub mod ai;
pub mod billing;
pub mod db;
pub mod pro;

pub use ai::{
    AiBackendConfig, AiChatRequest, AiChatResponse, AiDbaCopilot, AiExplainRequest,
    AiExplainResponse, SchemaContext,
};
pub use connection::{ConnectionConfig, DriverKind, Environment};
pub use driver::DriverRegistry;
pub use error::{CatermError, CatableError};
pub use executor::{ConnectionTestResult, QueryExecutor, QueryResult, TableInfo};
pub use guard::{GuardVerdict, GuardrailEngine, QueryRisk};
pub use vault::{
    change_master_password, is_vault_initialized, reset_vault, validate_vault_password,
    MIN_PASSWORD_LEN, Vault,
};
