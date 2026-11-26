//! SSH host key verification.
//!
//! A server is used only when its key is trusted: either pinned by the user in CATable
//! (`ssh_hosts.json`) or already present in their OpenSSH `~/.ssh/known_hosts`. The check runs
//! after the handshake and **before** any password, key or agent authentication.
//!
//! Failures carry a machine-readable marker the UI turns into a question:
//! `SSH_HOST_UNKNOWN [host|port|key-type|SHA256:…]` or `SSH_HOST_CHANGED [...]`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use ssh2::{CheckResult, HashType, KnownHostFileKind, Session};

use crate::error::CatableError;
use crate::paths::app_data_dir;

const STORE_FILE: &str = "ssh_hosts.json";
static WRITE_LOCK: Mutex<()> = parking_lot::const_mutex(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PinnedHost {
    pub key_type: String,
    pub fingerprint: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Trusted,
    Unknown,
    Changed,
}

fn host_id(host: &str, port: u16) -> String {
    format!("{}:{port}", host.trim().to_ascii_lowercase())
}

/// A pinned key decides on its own; without one, the user's OpenSSH file can vouch.
fn decide(pinned: Option<&PinnedHost>, presented: &str, in_openssh_file: bool) -> Verdict {
    match pinned {
        Some(p) if p.fingerprint == presented => Verdict::Trusted,
        Some(_) => Verdict::Changed,
        None if in_openssh_file => Verdict::Trusted,
        None => Verdict::Unknown,
    }
}

fn load(path: &Path) -> Result<BTreeMap<String, PinnedHost>, CatableError> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            CatableError::Tunnel(format!("{} is damaged ({e}); SSH is blocked until it is fixed", path.display()))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(CatableError::Tunnel(format!("Cannot read trusted SSH hosts: {e}"))),
    }
}

fn save(path: &Path, hosts: &BTreeMap<String, PinnedHost>) -> Result<(), CatableError> {
    let fail = |e: std::io::Error| CatableError::Tunnel(format!("Cannot save trusted SSH hosts: {e}"));
    let json = serde_json::to_vec_pretty(hosts)
        .map_err(|e| CatableError::Serialization(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(fail)?;
    std::fs::rename(&tmp, path).map_err(fail)
}

fn store_path() -> Result<PathBuf, CatableError> {
    Ok(app_data_dir()?.join(STORE_FILE))
}

/// Records the key the user approved for `host:port`.
pub fn trust(host: &str, port: u16, key_type: &str, fingerprint: &str) -> Result<(), CatableError> {
    if !fingerprint.starts_with("SHA256:") || fingerprint.len() < 20 {
        return Err(CatableError::Tunnel("Not a SHA-256 host key fingerprint".into()));
    }
    let _g = WRITE_LOCK.lock();
    let path = store_path()?;
    let mut hosts = load(&path)?;
    hosts.insert(
        host_id(host, port),
        PinnedHost { key_type: key_type.to_string(), fingerprint: fingerprint.to_string() },
    );
    save(&path, &hosts)
}

/// Removes the pin, e.g. after a server was legitimately reinstalled.
pub fn forget(host: &str, port: u16) -> Result<(), CatableError> {
    let _g = WRITE_LOCK.lock();
    let path = store_path()?;
    let mut hosts = load(&path)?;
    hosts.remove(&host_id(host, port));
    save(&path, &hosts)
}

fn openssh_vouches(session: &Session, host: &str, port: u16, key: &[u8]) -> bool {
    let Some(file) = dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts")) else {
        return false;
    };
    let Ok(mut known) = session.known_hosts() else {
        return false;
    };
    if known.read_file(&file, KnownHostFileKind::OpenSSH).is_err() {
        return false;
    }
    matches!(known.check_port(host, port, key), CheckResult::Match)
}

/// Must be called after `handshake()` and before authenticating.
pub fn verify(session: &Session, host: &str, port: u16) -> Result<(), CatableError> {
    let (key, key_type) = session
        .host_key()
        .ok_or_else(|| CatableError::Tunnel("The SSH server presented no host key".into()))?;
    let digest = session
        .host_key_hash(HashType::Sha256)
        .ok_or_else(|| CatableError::Tunnel("Cannot fingerprint the SSH host key".into()))?;
    let fingerprint = format!("SHA256:{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest));
    let key_type = format!("{key_type:?}").to_ascii_lowercase();

    let pinned = load(&store_path()?)?.remove(&host_id(host, port));
    let vouched = pinned.is_none() && openssh_vouches(session, host, port, key);
    let marker = match decide(pinned.as_ref(), &fingerprint, vouched) {
        Verdict::Trusted => return Ok(()),
        Verdict::Unknown => "SSH_HOST_UNKNOWN",
        Verdict::Changed => "SSH_HOST_CHANGED",
    };
    Err(CatableError::Tunnel(format!("{marker} [{host}|{port}|{key_type}|{fingerprint}]")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(fp: &str) -> PinnedHost {
        PinnedHost { key_type: "ed25519".into(), fingerprint: fp.into() }
    }

    #[test]
    fn only_a_matching_or_vouched_key_is_trusted() {
        assert_eq!(decide(Some(&pin("SHA256:a")), "SHA256:a", false), Verdict::Trusted);
        assert_eq!(decide(None, "SHA256:a", true), Verdict::Trusted);
        assert_eq!(decide(None, "SHA256:a", false), Verdict::Unknown);
    }

    #[test]
    fn a_different_key_is_rejected_even_if_openssh_would_accept_it() {
        assert_eq!(decide(Some(&pin("SHA256:a")), "SHA256:b", true), Verdict::Changed);
    }

    #[test]
    fn store_roundtrips_and_a_damaged_file_blocks_instead_of_trusting() {
        let dir = std::env::temp_dir().join(format!("catable-hosts-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(STORE_FILE);
        assert!(load(&path).unwrap().is_empty());
        let mut hosts = BTreeMap::new();
        hosts.insert(host_id("Bastion.Example", 22), pin("SHA256:abc"));
        save(&path, &hosts).unwrap();
        assert_eq!(load(&path).unwrap().get("bastion.example:22"), Some(&pin("SHA256:abc")));
        std::fs::write(&path, b"not json").unwrap();
        assert!(load(&path).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
