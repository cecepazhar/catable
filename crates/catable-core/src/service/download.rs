// Auto-download & SHA256 provisioning of portable database engine binaries
// Per-OS: Windows -> zip, Linux/macOS -> tar.gz
// (Phase 2 — CAT-SVC-005/006/007)

use crate::error::CatableError;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetOs {
    Linux,
    Macos,
    Windows,
}

impl TargetOs {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }

    pub fn archive_ext(&self) -> &'static str {
        match self {
            Self::Windows => "zip",
            Self::Linux | Self::Macos => "tar.gz",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ServiceVersion {
    pub engine: crate::service::EngineType,
    pub version: String,
    /// SHA-256 (hex, 64 chars) of the downloaded archive. Verified before extraction.
    pub sha256: String,
    /// Base download URL (e.g. https://packages.fathforce.com/catable-engines/...)
    pub url: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ServiceProvisionResult {
    pub bin_dir: std::path::PathBuf,
    pub data_dir: std::path::PathBuf,
    pub initialized: bool,
}

/// Download, verify (SHA-256), extract and init the engine binaries into `bin_dir`.
///
/// # Security (Zero-Knowledge / Sovereign CA ethos)
/// - Every engine archive MUST carry a known-good SHA-256; mismatch aborts immediately.
/// - Binaries are placed under the user's `db-engines/` tree and never touch /usr.
/// - No vendor telemetry.
pub fn download_service(
    v: &ServiceVersion,
    bin_dir: &Path,
    data_dir: &Path,
) -> Result<ServiceProvisionResult, CatableError> {
    use std::io::Read;

    std::fs::create_dir_all(bin_dir)
        .map_err(|e| CatableError::Io(format!("bin_dir: {}", e)))?;
    std::fs::create_dir_all(data_dir)
        .map_err(|e| CatableError::Io(format!("data_dir: {}", e)))?;

    let archive_name = format!("engine-{}-{}.{}", v.engine, v.version, v.engine_archive_name());
    let archive_path = bin_dir.join(&archive_name);

    // 1) Download via ureq (blocking, in-worker thread)
    let response = ureq::get(&v.url)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| CatableError::Io(format!("download {} failed: {}", v.url, e)))?;
    let mut bytes = Vec::new();
    response.body_mut().read_to_end(&mut bytes)
        .map_err(|e| CatableError::Io(format!("read body: {}", e)))?;

    // 2) SHA-256 verify
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let got: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    if !got.eq_ignore_ascii_case(v.sha256.trim()) {
        return Err(CatableError::Io(format!(
            "SHA-256 mismatch for {}: expected {} got {}",
            archive_name, v.sha256, got
        )));
    }

    // 3) Write archive + extract
    let archive_path = bin_dir.join(&archive_name);
    std::fs::write(&archive_path, &bytes)
        .map_err(|e| CatableError::Io(format!("write archive: {}", e)))?;
    extract(&archive_path, bin_dir, v)?;

    // 4) init
    let initialized = init_engine(v, bin_dir, data_dir)?;

    Ok(ServiceProvisionResult {
        bin_dir: bin_dir.to_path_buf(),
        data_dir: data_dir.to_path_buf(),
        initialized,
    })
}

fn extract(archive: &Path, dest: &Path, v: &ServiceVersion) -> Result<(), CatableError> {
    use std::fs::File;
    use std::io::Read;

    let file = File::open(archive).map_err(|e| CatableError::Io(e.to_string()))?;
    let is_zip = archive
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("zip"))
        .unwrap_or(false);

    if is_zip {
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|e| CatableError::Io(format!("bad zip: {}", e)))?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| CatableError::Io(e.to_string()))?;
            let name = entry.name().to_string();
            // Zip-slip guard
            if name.starts_with("..") || name.starts_with('/') {
                continue;
            }
            let out_path = dest.join(&name);
            if entry.is_dir() {
                std::fs::create_dir_all(out_path).ok();
            } else {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf).ok();
                std::fs::write(&out_path, buf).map_err(|e| CatableError::Io(e.to_string()))?;
                #[cfg(unix)]
                std::os::unix::fs::PermissionsExt::set_permissions(
                    &out_path,
                    std::fs::Permissions::from_mode(0o755),
                )
                .ok();
            }
        }
    } else {
        let gz = flate2::read::GzDecoder::new(file);
        let tar = tar::Archive::new(gz);
        let mut entries = tar
            .into_entries()
            .map_err(|e| CatableError::Io(e.to_string()))?;
        while let Some(mut entry) = entries
            .next()
            .ok_or_else(|| CatableError::Io("tar iteration failed".into()))?
        {
            let name = entry.path().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            if name.is_empty() || name.starts_with("..") {
                continue;
            }
            let out_path = dest.join(&name);
            if entry.header().entry_type().is_dir() {
                std::fs::create_dir_all(out_path).ok();
            } else {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                entry
                    .unpack(out_path)
                    .map_err(|e| CatableError::Io(format!("tar unpack: {}", e)))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if !name.ends_with('/') && !name.contains(".conf") {
                        std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(0o755)).ok();
                    }
                }
            }
        }
    }
    let _ = v; // v is for context
    Ok(())
}

fn init_engine(v: &ServiceVersion, bin_dir: &Path, data_dir: &Path) -> Result<bool, CatableError> {
    use std::process::Command;

    let marker = data_dir.join(".catable_initialized");
    if marker.exists() {
        return Ok(true);
    }

    let mut ok = false;
    match v.engine {
        crate::service::EngineType::Postgres => {
            let initdb = bin_dir.join("initdb");
            if initdb.exists() {
                let out = Command::new(&initdb)
                    .args(["-D", &data_dir.to_string_lossy(), "-U", "catable", "--auth=trust"])
                    .output();
                match out {
                    Ok(o) if o.status.success() => {
                        std::fs::write(&marker, "1").ok();
                        ok = true;
                    }
                    Ok(o) => return Err(CatableError::Io(format!(
                        "initdb failed: {}",
                        String::from_utf8_lossy(&o.stderr)
                    ))),
                    Err(e) => return Err(CatableError::Io(format!("initdb spawn: {}", e))),
                }
            }
        }
        crate::service::EngineType::Mysql | crate::service::EngineType::Mariadb => {
            let init = bin_dir.join("mysql_install_db");
            if init.exists() {
                let out = Command::new(&init)
                    .args(["--datadir", &data_dir.to_string_lossy(), "--skip-test-db"])
                    .output();
                match out {
                    Ok(o) if o.status.success() => {
                        std::fs::write(&marker, "1").ok();
                        ok = true;
                    }
                    Ok(o) => return Err(CatableError::Io(format!(
                        "mysql_install_db failed: {}",
                        String::from_utf8_lossy(&o.stderr)
                    ))),
                    Err(e) => return Err(CatableError::Io(format!("mysql_install_db spawn: {}", e))),
                }
            }
        }
    }
    Ok(ok)
}

// engine-specific archive filename
trait EngineArchive {
    fn engine_archive_name(&self) -> String;
}
impl EngineArchive for ServiceVersion {
    fn engine_archive_name(&self) -> String {
        format!(
            "{}-{}-{}",
            self.engine,
            self.version,
            TargetOs::current()
        )
        .to_lowercase()
    }
}
impl std::fmt::Display for TargetOs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetOs::Linux => write!(f, "linux"),
            TargetOs::Macos => write!(f, "macos"),
            TargetOs::Windows => write!(f, "windows"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_ext_matches_os() {
        let ext = TargetOs::Windows.archive_ext();
        assert_eq!(ext, "zip");
        let ext = TargetOs::Linux.archive_ext();
        assert_eq!(ext, "tar.gz");
    }

    #[test]
    fn current_os_is_one_of() {
        let os = TargetOs::current();
        assert!(matches!(os, TargetOs::Linux | TargetOs::Macos | TargetOs::Windows));
    }
}
