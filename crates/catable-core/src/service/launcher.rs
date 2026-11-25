// Native process supervision for portable DB engines (init / launch / stop).
// Windows: every child runs with CREATE_NO_WINDOW so no console flashes.

use crate::error::CatableError;
use crate::service::catalog::{log_path, DbService, EngineType};
use crate::service::download::{find_exe, ProgressFn, ServiceProgress};
use crate::service::monitor::is_port_open;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    pub pid: u32,
    pub log_file: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopResult {
    pub stopped: bool,
    pub forced: bool,
}

fn command(exe: &Path) -> Command {
    let mut c = Command::new(exe);
    c.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

fn need_exe(svc: &DbService, names: &[&str]) -> Result<PathBuf, CatableError> {
    find_exe(&svc.bin_dir, names).ok_or_else(|| {
        CatableError::Io(format!(
            "{} {} binaries are incomplete ({} not found). Remove and re-download the engine.",
            svc.engine,
            svc.version,
            names[0]
        ))
    })
}

/// Root of an engine install (parent of its `bin` folder).
fn base_dir(exe: &Path) -> PathBuf {
    let bin = exe.parent().unwrap_or(exe);
    if bin.file_name().map(|n| n.eq_ignore_ascii_case("bin")).unwrap_or(false) {
        bin.parent().unwrap_or(bin).to_path_buf()
    } else {
        bin.to_path_buf()
    }
}

fn open_log(path: &Path) -> Result<(Stdio, Stdio), CatableError> {
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| CatableError::Io(e.to_string()))?;
    let f2 = f.try_clone().map_err(|e| CatableError::Io(e.to_string()))?;
    Ok((Stdio::from(f), Stdio::from(f2)))
}

/// Last `n` lines of a log file (best-effort).
pub fn tail_log(path: &Path, n: usize) -> String {
    let Ok(bytes) = std::fs::read(path) else { return String::new() };
    let start = bytes.len().saturating_sub(64 * 1024);
    let text = String::from_utf8_lossy(&bytes[start..]);
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn run_logged(mut cmd: Command, log: &Path, what: &str) -> Result<(), CatableError> {
    let (o, e) = open_log(log)?;
    let status = cmd
        .stdout(o)
        .stderr(e)
        .status()
        .map_err(|e| CatableError::Io(format!("{what}: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(CatableError::Io(format!(
            "{what} failed ({status}).\n{}",
            tail_log(log, 12)
        )))
    }
}

fn is_initialized(svc: &DbService) -> bool {
    match svc.engine {
        EngineType::Postgres => svc.data_dir.join("PG_VERSION").exists(),
        EngineType::Mysql | EngineType::Mariadb => svc.data_dir.join("mysql").is_dir(),
        EngineType::Redis => true,
    }
}

/// Initialize the data directory on first start. Returns `true` when a fresh
/// cluster was created (caller then creates the default database).
pub fn init_data_dir(svc: &DbService, progress: ProgressFn<'_>) -> Result<bool, CatableError> {
    if is_initialized(svc) {
        return Ok(false);
    }
    let log = log_path(&svc.id)?;
    // Engines insist on an empty (or absent) data dir.
    if svc.data_dir.exists() {
        let _ = std::fs::remove_dir_all(&svc.data_dir);
    }
    progress(ServiceProgress::msg("init", format!("Initializing {} data directory", svc.engine)));

    match svc.engine {
        EngineType::Postgres => {
            let initdb = need_exe(svc, &["initdb"])?;
            let mut c = command(&initdb);
            c.arg("-D").arg(&svc.data_dir)
                .args(["-U", "postgres", "-A", "trust", "-E", "UTF8", "--locale=C"]);
            run_logged(c, &log, "initdb")?;
        }
        EngineType::Mysql => {
            let mysqld = need_exe(svc, &["mysqld"])?;
            std::fs::create_dir_all(&svc.data_dir).map_err(|e| CatableError::Io(e.to_string()))?;
            let mut c = command(&mysqld);
            c.arg("--no-defaults")
                .arg("--initialize-insecure")
                .arg(format!("--basedir={}", base_dir(&mysqld).display()))
                .arg(format!("--datadir={}", svc.data_dir.display()))
                .arg("--console");
            run_logged(c, &log, "mysqld --initialize").map_err(|e| {
                CatableError::Io(format!(
                    "{e}\nHint: MySQL needs the Microsoft Visual C++ 2015-2022 Redistributable (x64)."
                ))
            })?;
        }
        EngineType::Mariadb => {
            let install = need_exe(svc, &["mariadb-install-db", "mysql_install_db"])?;
            let mut c = command(&install);
            c.arg(format!("--datadir={}", svc.data_dir.display()))
                .arg(format!("--port={}", svc.port));
            run_logged(c, &log, "mariadb-install-db")?;
        }
        EngineType::Redis => {}
    }
    Ok(true)
}

/// Spawn the engine and block until its TCP port accepts connections.
pub fn launch_service(svc: &DbService, progress: ProgressFn<'_>) -> Result<LaunchResult, CatableError> {
    if is_port_open(svc.port, Duration::from_millis(200)) {
        return Err(CatableError::Io(format!(
            "Port {} is already in use by another process. Change the service port and try again.",
            svc.port
        )));
    }
    let log = log_path(&svc.id)?;
    let _ = std::fs::write(&log, format!("=== CATable: starting {} {} on 127.0.0.1:{} ===\n", svc.engine, svc.version, svc.port));
    std::fs::create_dir_all(&svc.data_dir).map_err(|e| CatableError::Io(e.to_string()))?;
    let (out, err) = open_log(&log)?;
    progress(ServiceProgress::msg("start", format!("Starting {} on port {}", svc.engine, svc.port)));

    let mut cmd = match svc.engine {
        EngineType::Postgres => {
            let exe = need_exe(svc, &["postgres"])?;
            let mut c = command(&exe);
            c.arg("-D").arg(&svc.data_dir)
                .arg("-p").arg(svc.port.to_string())
                .args(["-c", "listen_addresses=127.0.0.1"]);
            c
        }
        EngineType::Mysql => {
            let exe = need_exe(svc, &["mysqld"])?;
            let mut c = command(&exe);
            c.arg("--no-defaults")
                .arg(format!("--basedir={}", base_dir(&exe).display()))
                .arg(format!("--datadir={}", svc.data_dir.display()))
                .arg(format!("--port={}", svc.port))
                .arg("--bind-address=127.0.0.1")
                .arg("--mysqlx=OFF")
                .arg("--console");
            c
        }
        EngineType::Mariadb => {
            let exe = need_exe(svc, &["mariadbd", "mysqld"])?;
            let mut c = command(&exe);
            let ini = svc.data_dir.join("my.ini");
            if ini.exists() {
                c.arg(format!("--defaults-file={}", ini.display()));
            } else {
                c.arg("--no-defaults");
            }
            c.arg(format!("--datadir={}", svc.data_dir.display()))
                .arg(format!("--port={}", svc.port))
                .arg("--bind-address=127.0.0.1")
                .arg("--console");
            c
        }
        EngineType::Redis => {
            let exe = need_exe(svc, &["redis-server"])?;
            // Config file avoids redis' space-splitting of argv paths.
            let conf = svc.data_dir.join("redis.conf");
            let body = format!(
                "port {}\nbind 127.0.0.1\ndir ./\nlogfile \"\"\nsave 900 1\nsave 300 10\nappendonly no\n",
                svc.port
            );
            std::fs::write(&conf, body).map_err(|e| CatableError::Io(e.to_string()))?;
            let mut c = command(&exe);
            c.current_dir(&svc.data_dir).arg("redis.conf");
            c
        }
    };

    let mut child: Child = cmd
        .stdout(out)
        .stderr(err)
        .spawn()
        .map_err(|e| CatableError::Io(format!("Failed to launch {}: {e}", svc.engine)))?;
    let pid = child.id();

    let budget = match svc.engine {
        EngineType::Redis => Duration::from_secs(15),
        EngineType::Postgres => Duration::from_secs(45),
        _ => Duration::from_secs(90),
    };
    let start = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(CatableError::Io(format!(
                "{} exited during startup ({status}).\n{}",
                svc.engine,
                tail_log(&log, 15)
            )));
        }
        if is_port_open(svc.port, Duration::from_millis(250)) {
            break;
        }
        if start.elapsed() > budget {
            kill_pid(pid);
            return Err(CatableError::Io(format!(
                "{} did not become ready within {}s.\n{}",
                svc.engine,
                budget.as_secs(),
                tail_log(&log, 15)
            )));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    // Dropping `Child` does not kill the process; it keeps running detached.
    drop(child);
    Ok(LaunchResult { pid, log_file: log })
}

/// Create the default application database right after first init.
pub fn create_default_database(svc: &DbService) {
    if svc.database.is_empty() {
        return;
    }
    let log = match log_path(&svc.id) {
        Ok(l) => l,
        Err(_) => return,
    };
    let port = svc.port.to_string();
    let port = port.as_str();
    let cmd = match svc.engine {
        EngineType::Postgres => find_exe(&svc.bin_dir, &["createdb"]).map(|exe| {
            let mut c = command(&exe);
            c.args(["-h", "127.0.0.1", "-p", port, "-U", "postgres", svc.database.as_str()]);
            c
        }),
        EngineType::Mysql | EngineType::Mariadb => {
            find_exe(&svc.bin_dir, &["mariadb", "mysql"]).map(|exe| {
                let mut c = command(&exe);
                c.args(["--protocol=TCP", "-h", "127.0.0.1", "-P", port, "-u", "root", "-e"])
                    .arg(format!("CREATE DATABASE IF NOT EXISTS `{}`", svc.database));
                c
            })
        }
        EngineType::Redis => None,
    };
    if let Some(c) = cmd {
        let _ = run_logged(c, &log, "create default database");
    }
}

fn wait_port_closed(port: u16, budget: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < budget {
        if !is_port_open(port, Duration::from_millis(150)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

pub fn kill_pid(pid: u32) {
    #[cfg(windows)]
    {
        let _ = command(Path::new("taskkill"))
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

fn postmaster_pid(data_dir: &Path) -> Option<u32> {
    std::fs::read_to_string(data_dir.join("postmaster.pid"))
        .ok()?
        .lines()
        .next()?
        .trim()
        .parse()
        .ok()
}

/// Graceful stop with engine-native tooling, falling back to a forced kill.
pub fn stop_service(svc: &DbService) -> Result<StopResult, CatableError> {
    if !is_port_open(svc.port, Duration::from_millis(200)) && svc.pid.is_none() {
        return Ok(StopResult { stopped: true, forced: false });
    }
    let port = svc.port.to_string();
    let port = port.as_str();
    let graceful = match svc.engine {
        EngineType::Postgres => find_exe(&svc.bin_dir, &["pg_ctl"]).map(|exe| {
            let mut c = command(&exe);
            c.arg("-D").arg(&svc.data_dir).args(["stop", "-m", "fast", "-w", "-t", "20"]);
            c
        }),
        EngineType::Mysql | EngineType::Mariadb => {
            find_exe(&svc.bin_dir, &["mariadb-admin", "mysqladmin"]).map(|exe| {
                let mut c = command(&exe);
                c.args(["--protocol=TCP", "-h", "127.0.0.1", "-P", port, "-u", "root", "shutdown"]);
                c
            })
        }
        EngineType::Redis => find_exe(&svc.bin_dir, &["redis-cli"]).map(|exe| {
            let mut c = command(&exe);
            c.args(["-h", "127.0.0.1", "-p", port, "shutdown"]);
            c
        }),
    };

    if let Some(mut c) = graceful {
        let _ = c.stdout(Stdio::null()).stderr(Stdio::null()).status();
        if wait_port_closed(svc.port, Duration::from_secs(20)) {
            return Ok(StopResult { stopped: true, forced: false });
        }
    }

    let pid = svc.pid.or_else(|| {
        if svc.engine == EngineType::Postgres { postmaster_pid(&svc.data_dir) } else { None }
    });
    if let Some(pid) = pid {
        kill_pid(pid);
    }
    let stopped = wait_port_closed(svc.port, Duration::from_secs(5));
    Ok(StopResult { stopped, forced: true })
}
