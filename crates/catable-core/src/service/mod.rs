//! DB Service Engine Manager — DBngin-style local database servers.
//!
//! Catalog of portable engine builds, per-service data dirs, streamed
//! download + extraction with progress, process launch/stop without console
//! windows, readiness probing, state reconciliation and auto-start.

mod catalog;
mod download;
mod launcher;
mod monitor;
mod port;
mod service;

pub use catalog::{
    find_release, sanitize_db_name, DbService, EngineRelease, EngineType, ServiceCatalog,
    ServiceState, RELEASES,
};
pub use download::{find_exe, ServiceProgress};
pub use launcher::{LaunchResult, StopResult};
pub use monitor::{is_port_open, probe_detail, probe_service, ProbeResult, ProbeState};
pub use port::{find_free_port, port_is_free};
pub use service::{
    autostart, create_service, engine_versions, get, list, logs, remove, restart, start, stop,
    stop_all, suggest_port, update, EngineVersionInfo,
};
