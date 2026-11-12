//! DB Service Engine Manager — native local database server supervision.
//!
//! Implements CATable's DBNGIN-style service catalog, dynamic port allocation,
//! binary provisioning (auto-download + SHA256), process launch/stop, and health monitoring.
//! Cross-platform: Linux, macOS, Windows.
//!
//! See task-db-service-engine.md in Obsidian Vault for full roadmap.

mod catalog;
#[cfg(not(windows))]
mod download;
mod launcher;
mod monitor;
mod port;
mod service;

pub use catalog::{AVAILABLE_VERSIONS, DbService, EngineType, ServiceCatalog, ServiceState};
#[cfg(not(windows))]
pub use download::{download_service, ServiceProvisionResult, ServiceVersion, TargetOs};
pub use launcher::{launch_service, LaunchResult, StopResult};
pub use monitor::{probe_detail, probe_service, ProbeResult, ProbeState};
pub use port::find_free_port;
pub use service::{create_and_start, create_service, get, list, remove, stop};
