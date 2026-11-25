// High-level ServiceManager — public API used by Tauri commands.

use crate::error::CatableError;
use crate::service::catalog::{
    engine_install_dir, find_release, log_path, DbService, EngineType, ServiceCatalog,
    ServiceState, RELEASES,
};
use crate::service::download::{ensure_installed, is_installed, ProgressFn, ServiceProgress};
use crate::service::launcher::{
    create_default_database, init_data_dir, launch_service, stop_service, tail_log, StopResult,
};
use crate::service::monitor::is_port_open;
use crate::service::port::port_is_free;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashSet;
use std::time::Duration;

/// Services with an operation in flight (start/stop) — excluded from state sync.
static BUSY: Mutex<Option<HashSet<String>>> = parking_lot::const_mutex(None);

struct BusyGuard(String);
impl BusyGuard {
    fn acquire(id: &str) -> Result<Self, CatableError> {
        let mut g = BUSY.lock();
        let set = g.get_or_insert_with(HashSet::new);
        if !set.insert(id.to_string()) {
            return Err(CatableError::Io("Another operation is already running for this service".into()));
        }
        Ok(Self(id.to_string()))
    }
}
impl Drop for BusyGuard {
    fn drop(&mut self) {
        if let Some(set) = BUSY.lock().as_mut() {
            set.remove(&self.0);
        }
    }
}
fn is_busy(id: &str) -> bool {
    BUSY.lock().as_ref().map(|s| s.contains(id)).unwrap_or(false)
}

fn not_found(id: &str) -> CatableError {
    CatableError::ConnectionNotFound(format!("service {id}"))
}

/// One selectable engine build for the "New Service" form.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineVersionInfo {
    pub engine: EngineType,
    pub version: String,
    pub size_bytes: u64,
    pub installed: bool,
    pub default_port: u16,
}

pub fn engine_versions() -> Vec<EngineVersionInfo> {
    RELEASES
        .iter()
        .map(|r| EngineVersionInfo {
            engine: r.engine,
            version: r.version.to_string(),
            size_bytes: r.size_bytes,
            installed: engine_install_dir(r.engine, r.version)
                .map(|d| is_installed(&d))
                .unwrap_or(false),
            default_port: r.engine.default_port(),
        })
        .collect()
}

/// Create a service definition. Port is validated against the catalog and the OS.
pub fn create_service(
    engine: EngineType,
    version: &str,
    name: &str,
    port: u16,
    auto_start: bool,
) -> Result<DbService, CatableError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CatableError::Io("Service name is required".into()));
    }
    if find_release(engine, version).is_none() {
        return Err(CatableError::Io(format!("{engine} {version} is not available")));
    }
    if port < 1024 {
        return Err(CatableError::Io("Port must be between 1024 and 65535".into()));
    }
    ServiceCatalog::update(|cat| {
        if cat.services.iter().any(|s| s.name.eq_ignore_ascii_case(name)) {
            return Err(CatableError::Io(format!("A service named \"{name}\" already exists")));
        }
        if cat.port_taken(port, None) {
            return Err(CatableError::Io(format!("Port {port} is already assigned to another service")));
        }
        let svc = cat.build_service(engine, version, name, port, auto_start)?;
        cat.services.push(svc.clone());
        Ok(svc)
    })
}

/// Suggest a port that is free in the catalog *and* on the OS.
pub fn suggest_port(engine: EngineType) -> u16 {
    let cat = ServiceCatalog::load_from_disk();
    let mut p = engine.default_port();
    while (cat.port_taken(p, None) || !port_is_free(p)) && p < u16::MAX {
        p += 1;
    }
    p
}

fn set_state(id: &str, state: ServiceState, pid: Option<u32>, err: Option<String>) -> Result<DbService, CatableError> {
    ServiceCatalog::update(|cat| {
        let s = cat.get_mut(id).ok_or_else(|| not_found(id))?;
        s.state = state;
        s.pid = pid;
        s.last_error = err;
        Ok(s.clone())
    })
}

/// Download (if needed) → init (first run) → launch → wait ready.
pub fn start(id: &str, progress: ProgressFn<'_>) -> Result<DbService, CatableError> {
    let _busy = BusyGuard::acquire(id)?;
    let svc = get(id)?;
    if svc.state == ServiceState::Running && is_port_open(svc.port, Duration::from_millis(200)) {
        return Ok(svc);
    }
    set_state(id, ServiceState::Starting, None, None)?;

    match start_inner(&svc, progress) {
        Ok(s) => Ok(s),
        Err(e) => {
            let msg = e.to_string();
            let _ = set_state(id, ServiceState::Failing, None, Some(msg.clone()));
            progress(ServiceProgress::msg("error", msg));
            Err(e)
        }
    }
}

fn start_inner(svc: &DbService, progress: ProgressFn<'_>) -> Result<DbService, CatableError> {
    let rel = find_release(svc.engine, &svc.version)
        .ok_or_else(|| CatableError::Io(format!("{} {} is not available", svc.engine, svc.version)))?;
    ensure_installed(rel, &svc.bin_dir, &mut *progress)?;
    let fresh = init_data_dir(svc, &mut *progress)?;
    let lr = launch_service(svc, &mut *progress)?;
    if fresh {
        progress(ServiceProgress::msg("init", format!("Creating database \"{}\"", svc.database)));
        create_default_database(svc);
    }
    let s = set_state(&svc.id, ServiceState::Running, Some(lr.pid), None)?;
    progress(ServiceProgress::msg("ready", format!("{} is running on port {}", s.engine, s.port)));
    Ok(s)
}

pub fn stop(id: &str) -> Result<StopResult, CatableError> {
    let _busy = BusyGuard::acquire(id)?;
    let svc = get(id)?;
    set_state(id, ServiceState::Stopping, svc.pid, None)?;
    match stop_service(&svc) {
        Ok(r) => {
            let state = if r.stopped { ServiceState::Stopped } else { ServiceState::Failing };
            let err = (!r.stopped).then(|| "Service did not stop cleanly".to_string());
            set_state(id, state, None, err)?;
            Ok(r)
        }
        Err(e) => {
            let _ = set_state(id, ServiceState::Failing, svc.pid, Some(e.to_string()));
            Err(e)
        }
    }
}

pub fn restart(id: &str, progress: ProgressFn<'_>) -> Result<DbService, CatableError> {
    stop(id)?;
    start(id, progress)
}

/// List services, reconciling persisted state with what is actually listening.
pub fn list() -> Result<Vec<DbService>, CatableError> {
    let snapshot = ServiceCatalog::load_from_disk().services;
    let mut changes: Vec<(String, ServiceState)> = Vec::new();
    for s in &snapshot {
        if is_busy(&s.id) {
            continue;
        }
        let open = is_port_open(s.port, Duration::from_millis(120));
        let next = match (s.state, open) {
            (ServiceState::Running, false) => Some(ServiceState::Stopped),
            (ServiceState::Starting | ServiceState::Stopping, true) => Some(ServiceState::Running),
            (ServiceState::Starting | ServiceState::Stopping, false) => Some(ServiceState::Stopped),
            (ServiceState::Stopped, true) if s.pid.is_some() => Some(ServiceState::Running),
            _ => None,
        };
        if let Some(n) = next {
            changes.push((s.id.clone(), n));
        }
    }
    if changes.is_empty() {
        return Ok(snapshot);
    }
    ServiceCatalog::update(|cat| {
        for (id, st) in &changes {
            if let Some(s) = cat.get_mut(id) {
                s.state = *st;
                if *st == ServiceState::Stopped {
                    s.pid = None;
                }
            }
        }
        Ok(cat.services.clone())
    })
}

pub fn get(id: &str) -> Result<DbService, CatableError> {
    ServiceCatalog::load_from_disk()
        .get(id)
        .cloned()
        .ok_or_else(|| not_found(id))
}

/// Edit name / port / auto-start. Port changes require the service to be stopped.
pub fn update(id: &str, name: Option<String>, port: Option<u16>, auto_start: Option<bool>) -> Result<DbService, CatableError> {
    ServiceCatalog::update(|cat| {
        if let Some(p) = port {
            if p < 1024 {
                return Err(CatableError::Io("Port must be between 1024 and 65535".into()));
            }
            if cat.port_taken(p, Some(id)) {
                return Err(CatableError::Io(format!("Port {p} is already assigned to another service")));
            }
        }
        let s = cat.get_mut(id).ok_or_else(|| not_found(id))?;
        if let Some(p) = port {
            if p != s.port && s.state != ServiceState::Stopped && s.state != ServiceState::Failing {
                return Err(CatableError::Io("Stop the service before changing its port".into()));
            }
            s.port = p;
        }
        if let Some(n) = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            s.name = n;
        }
        if let Some(a) = auto_start {
            s.auto_start = a;
        }
        Ok(s.clone())
    })
}

/// Remove a service; stops it first. `delete_data` wipes its data directory.
pub fn remove(id: &str, delete_data: bool) -> Result<(), CatableError> {
    let svc = get(id)?;
    if is_port_open(svc.port, Duration::from_millis(150)) && svc.state != ServiceState::Stopped {
        let _ = stop(id);
    }
    ServiceCatalog::update(|cat| {
        cat.services.retain(|s| s.id != id);
        Ok(())
    })?;
    if delete_data {
        let _ = std::fs::remove_dir_all(&svc.data_dir);
        if let Ok(l) = log_path(id) {
            let _ = std::fs::remove_file(l);
        }
    }
    Ok(())
}

pub fn logs(id: &str, lines: usize) -> Result<String, CatableError> {
    Ok(tail_log(&log_path(id)?, lines.clamp(10, 2000)))
}

/// Start every service flagged `auto_start` (called once at app launch).
pub fn autostart(progress: &mut dyn FnMut(&str, ServiceProgress)) {
    let ids: Vec<String> = ServiceCatalog::load_from_disk()
        .services
        .into_iter()
        .filter(|s| s.auto_start)
        .map(|s| s.id)
        .collect();
    for id in ids {
        let mut cb = |p: ServiceProgress| progress(&id, p);
        let _ = start(&id, &mut cb);
    }
}

/// Stop every running service (used by "Stop all").
pub fn stop_all() -> usize {
    let running: Vec<String> = list()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.state == ServiceState::Running)
        .map(|s| s.id)
        .collect();
    running.iter().filter(|id| stop(id).is_ok()).count()
}
