// Service health probing (fast loopback TCP checks).

use crate::error::CatableError;
use crate::service::catalog::DbService;
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
    Reachable,
    Refused,
    Timeout,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub reachable: bool,
    pub state: ProbeState,
    pub latency_ms: u64,
}

/// True when something accepts TCP connections on 127.0.0.1:`port`.
pub fn is_port_open(port: u16, timeout: Duration) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, timeout).is_ok()
}

/// Poll until the port opens or `timeout` elapses.
pub fn probe_service(svc: &DbService, timeout: Duration) -> Result<bool, CatableError> {
    Ok(probe_detail(svc, timeout)?.reachable)
}

pub fn probe_detail(svc: &DbService, timeout: Duration) -> Result<ProbeResult, CatableError> {
    let start = Instant::now();
    loop {
        let t = Instant::now();
        if is_port_open(svc.port, Duration::from_millis(300)) {
            return Ok(ProbeResult {
                reachable: true,
                state: ProbeState::Reachable,
                latency_ms: t.elapsed().as_millis() as u64,
            });
        }
        if start.elapsed() >= timeout {
            return Ok(ProbeResult {
                reachable: false,
                state: ProbeState::Refused,
                latency_ms: 0,
            });
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_port_is_not_open() {
        assert!(!is_port_open(1, Duration::from_millis(100)));
    }
}
