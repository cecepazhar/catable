// High-level ServiceManager — public API used by Tauri commands
// (Phase 4 — facade)

use crate::error::CatableError;
use crate::service::catalog::{DbService, EngineType, ServiceCatalog, ServiceState};
use crate::service::download::ServiceVersion;
use crate::service::launcher::{launch_service, stop_service, LaunchResult, StopResult};
use crate::service::monitor::probe_service;
use crate::service::port::find_free_port;
use std::time::Duration;

/// Create a service definition (allocates port + dirs, writes catalog.json).
pub fn create_service(
    engine: EngineType,
    version: &str,
    name: &str,
    preferred_port: u16,
) -> Result<DbService, CatableError> {
    let mut catalog = ServiceCatalog::load_from_disk();
    let preferred = find_free_port(preferred_port)?;
    catalog.add_service(engine, version, name, preferred)
}

/// Provision (download + init) then start a service in one call.
pub fn create_and_start(
    engine: EngineType,
    version: &str,
    name: &str,
    preferred_port: u16,
) -> Result<(DbService, LaunchResult), CatableError> {
    let svc = create_service(engine, version, name, preferred_port)?;

    let ver = ServiceVersion {
        engine,
        version: version.to_string(),
        sha256: String::new(), // populated by caller via register_binary_sha256()
        url: String::new(),
    };
    // NOTE: download_service() requires real binary URLs; in Phase 1-3 we
    // assume the engine binary is already provisioned (manual pre-stage or
    // Phase 2 real URL). Launch + readiness probe runs synchronously.
    let svc_now = svc.clone();
    let lr = launch_service(&svc_now, &ver)?;

    // Wait for port readiness (30 s budget)
    let ready = probe_service(&svc_now, Duration::from_secs(30))?;
    let mut cat = ServiceCatalog::load_from_disk();
    let final_state = if ready { ServiceState::Running } else { ServiceState::Failing };
    cat.update_state(&svc.id, final_state);
    let final_svc = cat.get(&svc.id).cloned().unwrap_or(svc_now);

    Ok((final_svc, lr))
}

/// Stop a running service.
pub fn stop(svc_id: &str) -> Result<StopResult, CatableError> {
    let mut catalog = ServiceCatalog::load_from_disk();
    let svc = catalog
        .get(svc_id)
        .cloned()
        .ok_or_else(|| CatableError::ConnectionNotFound(svc_id.to_string()))?;
    let res = stop_service(&svc)?;
    catalog.update_state(svc_id, ServiceState::Stopped);
    Ok(res)
}

/// List all services.
pub fn list() -> Result<Vec<DbService>, CatableError> {
    let catalog = ServiceCatalog::load_from_disk();
    Ok(catalog.services)
}

/// Get one service by id.
pub fn get(svc_id: &str) -> Result<DbService, CatableError> {
    let catalog = ServiceCatalog::load_from_disk();
    catalog
        .get(svc_id)
        .cloned()
        .ok_or_else(|| CatableError::ConnectionNotFound(svc_id.to_string()))
}

/// Remove a stopped service from the catalog.
pub fn remove(svc_id: &str) -> Result<(), CatableError> {
    let mut catalog = ServiceCatalog::load_from_disk();
    catalog.remove(svc_id).ok_or_else(|| CatableError::ConnectionNotFound(svc_id.to_string()))?;
    Ok(())
}
