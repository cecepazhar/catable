// Dynamic port allocation for local DB services
use crate::error::CatableError;
use std::net::TcpListener;

/// Find the first free TCP port starting from `preferred`.
/// Checks system OS availability.
pub fn find_free_port(preferred: u16) -> Result<u16, CatableError> {
    for p in preferred..(preferred + 100) {
        if let Ok(_listener) = TcpListener::bind(("127.0.0.1", p)) {
            return Ok(p);
        }
    }
    Err(CatableError::Io(format!(
        "No free ports found in range {}-{}",
        preferred,
        preferred + 99
    )))
}
