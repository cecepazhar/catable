// Service health probing (TCP connect + optional engine-specific check)
// (Phase 3 supplement — health monitor)

use crate::error::CatableError;
use crate::service::catalog::DbService;
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// Poll service TCP port readiness. `timeout` is how long to wait for the
/// port to become responsive. Returns true if service is reachable.
pub fn probe_service(svc: &DbService, timeout: Duration) -> Result<bool, CatableError> {
    let addr = format!("127.0.0.1:{}", svc.port);
    let start = Instant::now();

    loop {
        match TcpStream::connect(&addr) {
            Ok(_) => {
                if svc.engine != crate::service::EngineType::Postgres {
                    // MySQL/MariaDB: TCP connect to port usually means ready
                    // unless in handshake phase; give one extra 200ms
                    std::thread::sleep(Duration::from_millis(200));
                    if TcpStream::connect(&addr).is_ok() {
                        return Ok(true);
                    }
                }
                return Ok(true);
            }
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                // expected during startup
                if start.elapsed() > timeout {
                    return Ok(false);
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
