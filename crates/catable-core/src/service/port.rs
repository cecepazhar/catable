// Dynamic port allocation for local DB services
use crate::error::CatableError;
use std::net::TcpListener;

/// Whether the OS lets us bind 127.0.0.1:`port` right now.
pub fn port_is_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Find the first free TCP port starting from `preferred`.
pub fn find_free_port(preferred: u16) -> Result<u16, CatableError> {
    let end = preferred.saturating_add(200);
    (preferred..end).find(|p| port_is_free(*p)).ok_or_else(|| {
        CatableError::Io(format!("No free ports found in range {preferred}-{end}"))
    })
}
