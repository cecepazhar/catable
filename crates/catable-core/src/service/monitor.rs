// Service health probing (TCP connect + engine-specific version sniff)
// (Phase 3 supplement — health monitor)

use crate::error::CatableError;
use crate::service::catalog::DbService;
use std::io::Read;
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// TCP reachability state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
    Reachable,
    Refused,
    Timeout,
}

/// Result of a service health probe.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeResult {
    pub reachable: bool,
    pub state: ProbeState,
    /// If reachable, the service engine reported its version (best-effort).
    pub engine_version: Option<String>,
}

/// Poll service TCP port readiness. `timeout` is how long to wait for the
/// port to become responsive. Returns `Ok(true)` if the port accepts
/// connections, `Ok(false)` if it remains refused after `timeout`,
/// `Err` for non-refused I/O failures.
pub fn probe_service(svc: &DbService, timeout: Duration) -> Result<bool, CatableError> {
    let r = probe_detail(svc, timeout)?;
    Ok(r.reachable)
}

/// Detailed probe with engine version detection.
pub fn probe_detail(svc: &DbService, timeout: Duration) -> Result<ProbeResult, CatableError> {
    let addr = format!("127.0.0.1:{}", svc.port);
    let start = Instant::now();

    loop {
        match TcpStream::connect(&addr) {
            Ok(mut stream) => {
                stream.set_read_timeout(Some(Duration::from_millis(500))).ok();
                let version = match svc.engine {
                    crate::service::EngineType::Postgres => {
                        let mut buf = [0u8; 12];
                        stream.read(&mut buf).ok();
                        String::from_utf8_lossy(&buf).to_string()
                    }
                    _ => {
                        let mut buf = [0u8; 128];
                        stream.read(&mut buf).ok();
                        let s = String::from_utf8_lossy(&buf);
                        s.split('\0')
                            .filter(|p| p.len() > 4 && p.len() < 32 && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'))
                            .next()
                            .map(|v| v.to_string())
                    }
                };
                return Ok(ProbeResult {
                    reachable: true,
                    state: ProbeState::Reachable,
                    engine_version: if version.is_empty() { None } else { Some(version) },
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                if start.elapsed() > timeout {
                    return Ok(ProbeResult {
                        reachable: false,
                        state: ProbeState::Refused,
                        engine_version: None,
                    });
                }
                std::thread::sleep(Duration::from_millis(300));
            }
            Err(e) => {
                if start.elapsed() > timeout {
                    return Err(CatableError::Io(format!("probe {}: {}", svc.name, e)));
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_times_out_when_no_service() {
        let svc = DbService {
            id: "probe-test".into(),
            engine: crate::service::EngineType::Postgres,
            version: "16".into(),
            name: "probe-test".into(),
            port: 65432,
            data_dir: std::path::PathBuf::from("/tmp/probe-test"),
            bin_dir: std::path::PathBuf::from("/tmp/probe-test-bin"),
            state: crate::service::catalog::ServiceState::Stopped,
            created_at: 0,
        };
        let ready = probe_service(&svc, Duration::from_millis(200)).unwrap();
        assert!(!ready);
    }
}
