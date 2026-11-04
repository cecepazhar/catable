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
pub mod mutation;
pub mod schema;
pub mod session;
pub mod sql_builder;
pub mod tunnel;
pub mod vault;
pub mod ai;
pub mod billing;
pub mod db;
pub mod pro;
pub mod license_local;

pub use ai::{
    AiBackendConfig, AiChatRequest, AiChatResponse, AiDbaCopilot, AiExplainRequest,
    AiExplainResponse, SchemaContext,
};
pub use connection::{ConnectionConfig, DriverKind, Environment, SshAuthType, SshTunnelConfig, DEFAULT_CONNECT_TIMEOUT_SECS, SANDBOX_CONNECTION_ID};
pub use driver::DriverRegistry;
pub use error::{CatermError, CatableError, ConnectErrorKind};
pub use executor::{ConnectionTestResult, QueryExecutor, QueryResult, TableInfo};
pub use guard::{GuardVerdict, GuardrailEngine, QueryRisk};
pub use session::{BrowseResult, Session, SessionInfo, SessionManager};
pub use mutation::{
    ApplyResult, CellValue, ColumnChange, ColumnValue, KeyPart, RowChangeSet, RowDelete, RowInsert,
    RowUpdate,
};
pub use schema::{ColumnDetail, ForeignKey, IndexInfo, TableStructure};
pub use sql_builder::{BrowseRequest, Filter, FilterMode, FilterOp, OrderBy};
pub use tunnel::{open_tunnel, open_tunnel_async, test_ssh_tunnel, test_ssh_tunnel_report, SshTestResult, TunnelSession};
pub use vault::{
    change_master_password, is_vault_initialized, reset_vault, validate_vault_password,
    MIN_PASSWORD_LEN, PersonalizedGroupInput, PersonalizedGroupRecord, Vault,
    VaultBackupPayload, WorkspaceInput, WorkspaceRecord,
};
