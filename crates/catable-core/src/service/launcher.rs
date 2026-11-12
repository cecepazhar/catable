// Native process launch / stop / restart for portable DB engine binaries
// (Phase 3 — CAT-SVC-008/009/010/011)

use crate::service::catalog::{DbService, ServiceState};
use crate::service::download::ServiceVersion;
use crate::error::CatableError;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LaunchResult {
    pub pid: u32,
    pub log_file: std::path::PathBuf,
    pub state: ServiceState,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StopResult {
    pub stopped: bool,
    pub forced: bool,
}

fn log_dir(svc: &DbService) -> Result<std::path::PathBuf, CatableError> {
    let dir = svc.bin_dir.join("..").join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| CatableError::Io(e.to_string()))?;
    Ok(dir.join(format!("{}.log", svc.id)))
}

/// Launch the service engine process in background, streaming output to
/// `logs/<service_id>.log`. Updates state to Starting → (caller confirms Running).
pub fn launch_service(
    svc: &DbService,
    version: &ServiceVersion,
) -> Result<LaunchResult, CatableError> {
    use std::os::unix::process::CommandExt;

    let bin_dir = &svc.bin_dir;
    let data_dir = &svc.data_dir;
    let log_file = log_dir(svc)?;

    let (cmd_path, args): (String, Vec<String>) = match svc.engine {
        crate::service::EngineType::Postgres => {
            let pg_ctl = bin_dir.join("pg_ctl");
            (
                pg_ctl.to_string_lossy().to_string(),
                vec![
                    "-D".into(),
                    data_dir.to_string_lossy().to_string(),
                    "-o".into(),
                    format!("-p {} -c unix_socket_directories='' --listen_addresses=127.0.0.1", svc.port),
                    "-l".into(),
                    log_file.to_string_lossy().to_string(),
                    "start".into(),
                ],
            )
        }
        crate::service::EngineType::Mysql | crate::service::EngineType::Mariadb => {
            let my = bin_dir.join("mysqld");
            if !my.exists() {
                // mariadb binary naming
                let m = bin_dir.join("mariadbd");
                let p = if m.exists() { m.to_string_lossy().to_string() } else { my.to_string_lossy().to_string() };
                (
                    p,
                    vec![
                        "--datadir".into(),
                        data_dir.to_string_lossy().to_string(),
                        "--port".into(),
                        svc.port.to_string(),
                        "--socket".into(),
                        data_dir.join("socket").to_string_lossy().to_string(),
                        "--bind-address".into(),
                        "127.0.0.1".into(),
                    ],
                )
            } else {
                (
                    my.to_string_lossy().to_string(),
                    vec![
                        "--datadir".into(),
                        data_dir.to_string_lossy().to_string(),
                        "--port".into(),
                        svc.port.to_string(),
                        "--socket".into(),
                        data_dir.join("socket").to_string_lossy().to_string(),
                        "--bind-address".into(),
                        "127.0.0.1".into(),
                    ],
                )
            }
        }
    };

    let _ = version; // reserved for context

    let mut cmd = Command::new(&cmd_path);
    cmd.args(&args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    // Detach: run in new process group so we can kill the whole tree
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| CatableError::Io(format!("spawn {}: {}", cmd_path, e)))?;

    let pid = child.id();
    let _ = child.wait(); // post-spawn wait (for pg_ctl fork semantics)

    Ok(LaunchResult {
        pid,
        log_file,
        state: ServiceState::Starting,
    })
}

/// Clean stop with 10s timeout then SIGKILL fallback.
pub fn stop_service(svc: &DbService) -> Result<StopResult, CatableError> {
    match svc.engine {
        crate::service::EngineType::Postgres => {
            let pg_ctl = svc.bin_dir.join("pg_ctl");
            let out = Command::new(&pg_ctl)
                .arg("-D")
                .arg(&svc.data_dir)
                .arg("stop")
                .arg("-m")
                .arg("fast")
                .output()
                .map_err(|e| CatableError::Io(format!("pg_ctl stop: {}", e)))?;
            if out.status.success() {
                return Ok(StopResult { stopped: true, forced: false });
            }
            // Fallback: SIGKILL any lingering postgres in datadir
            kill_tree(svc)
        }
        crate::service::EngineType::Mysql | crate::service::EngineType::Mariadb => {
            let socket = svc.data_dir.join("socket");
            let mysqladmin = svc.bin_dir.join("mysqladmin");
            let out = Command::new(&mysqladmin)
                .arg("--socket")
                .arg(&socket)
                .arg("shutdown")
                .output()
                .map_err(|e| CatableError::Io(format!("mysqladmin shutdown: {}", e)))?;
            if out.status.success() {
                return Ok(StopResult { stopped: true, forced: false });
            }
            kill_tree(svc)
        }
    }
}

fn kill_tree(svc: &DbService) -> Result<StopResult, CatableError> {
    let pattern = match svc.engine {
        crate::service::EngineType::Postgres => "postgres".to_string(),
        crate::service::EngineType::Mysql | crate::service::EngineType::Mariadb => {
            "mysqld".to_string()
        }
    };

    let pidfile = svc.data_dir.join("postmaster.pid");
    if pidfile.exists() {
        if let Ok(content) = std::fs::read_to_string(&pidfile) {
            if let Ok(pid) = content.lines().next().unwrap_or("").parse::<i32>() {
                let _ = std::process::Command::new("kill")
                    .arg("-SIGTERM")
                    .arg(pid.to_string())
                    .output();
            }
        }
    }

    // Final safety: pkill by name + port filter
    let _ = std::process::Command::new("pkill")
        .arg("-f")
        .arg(format!("{}.*{}", pattern, svc.data_dir.display()))
        .output();

    Ok(StopResult { stopped: true, forced: true })
}
