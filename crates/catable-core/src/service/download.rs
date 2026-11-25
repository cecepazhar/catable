// Engine provisioning: streamed HTTPS download with progress + safe zip extraction.
// One shared install per engine+version under `db-engines/engines/<slug>-<ver>`.

use crate::error::CatableError;
use crate::service::catalog::EngineRelease;
use parking_lot::Mutex;
use serde::Serialize;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Progress event reported to the UI during long-running service operations.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceProgress {
    /// download | extract | init | start | stop | ready | error
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
    pub message: String,
}

impl ServiceProgress {
    pub fn msg(phase: &'static str, message: impl Into<String>) -> Self {
        Self { phase, done: 0, total: 0, message: message.into() }
    }
}

pub type ProgressFn<'a> = &'a mut dyn FnMut(ServiceProgress);

const MARKER: &str = ".catable-installed";

/// Serializes installs so two services never extract the same engine at once.
static INSTALL_LOCK: Mutex<()> = parking_lot::const_mutex(());

pub fn is_installed(install_dir: &Path) -> bool {
    install_dir.join(MARKER).exists()
}

/// Skip bulky, unused parts of vendor archives (GUI tools, docs, debug symbols).
fn skip_entry(rel: &Path) -> bool {
    let s = rel.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    const SKIP_DIRS: &[&str] = &["pgadmin 4/", "stackbuilder/", "/doc/", "/docs/", "/include/", "/symbols/", "/mysql-test/", "/sql-bench/"];
    if SKIP_DIRS.iter().any(|d| s.contains(d) || s.starts_with(d.trim_start_matches('/'))) {
        return true;
    }
    s.ends_with(".pdb") || s.ends_with(".lib") && s.contains("/lib/") && !s.contains("plugin")
}

/// Ensure the engine binaries exist in `install_dir`, downloading if needed.
pub fn ensure_installed(
    rel: &EngineRelease,
    install_dir: &Path,
    progress: ProgressFn<'_>,
) -> Result<(), CatableError> {
    if is_installed(install_dir) {
        return Ok(());
    }
    if !cfg!(windows) {
        return Err(CatableError::Io(
            "Automatic engine provisioning is currently available on Windows only".into(),
        ));
    }
    let _g = INSTALL_LOCK.lock();
    if is_installed(install_dir) {
        return Ok(());
    }

    let parent = install_dir
        .parent()
        .ok_or_else(|| CatableError::Io("invalid install dir".into()))?;
    std::fs::create_dir_all(parent).map_err(|e| CatableError::Io(e.to_string()))?;
    if install_dir.exists() {
        let _ = std::fs::remove_dir_all(install_dir);
    }
    let file_name = install_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "engine".into());
    let archive = parent.join(format!("{file_name}.zip.part"));

    download(rel, &archive, &mut *progress)?;
    let res = extract_zip(&archive, install_dir, &mut *progress);
    let _ = std::fs::remove_file(&archive);
    if let Err(e) = res {
        let _ = std::fs::remove_dir_all(install_dir);
        return Err(e);
    }
    std::fs::write(install_dir.join(MARKER), rel.url).map_err(|e| CatableError::Io(e.to_string()))?;
    Ok(())
}

fn download(rel: &EngineRelease, dest: &Path, progress: ProgressFn<'_>) -> Result<(), CatableError> {
    progress(ServiceProgress::msg("download", format!("Connecting to {}", host_of(rel.url))));
    let resp = ureq::get(rel.url)
        .header("User-Agent", concat!("CATable/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| CatableError::Io(format!("Download failed ({}): {e}", host_of(rel.url))))?;

    let total = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(rel.size_bytes);

    let mut body = resp.into_body();
    let mut reader = body.as_reader();
    let file = std::fs::File::create(dest).map_err(|e| CatableError::Io(e.to_string()))?;
    let mut out = BufWriter::with_capacity(1 << 20, file);
    let mut buf = vec![0u8; 256 * 1024];
    let mut done: u64 = 0;
    let mut last = Instant::now() - Duration::from_secs(1);

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| CatableError::Io(format!("Download interrupted: {e}")))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| CatableError::Io(e.to_string()))?;
        done += n as u64;
        if last.elapsed() >= Duration::from_millis(150) {
            last = Instant::now();
            progress(ServiceProgress {
                phase: "download",
                done,
                total,
                message: format!("Downloading {} {}", rel.engine, rel.version),
            });
        }
    }
    out.flush().map_err(|e| CatableError::Io(e.to_string()))?;
    progress(ServiceProgress { phase: "download", done, total: done.max(total), message: "Download complete".into() });
    Ok(())
}

fn host_of(url: &str) -> &str {
    url.trim_start_matches("https://").split('/').next().unwrap_or(url)
}

fn extract_zip(archive: &Path, dest: &Path, progress: ProgressFn<'_>) -> Result<(), CatableError> {
    let file = std::fs::File::open(archive).map_err(|e| CatableError::Io(e.to_string()))?;
    let mut zip = zip::ZipArchive::new(BufReader::with_capacity(1 << 20, file))
        .map_err(|e| CatableError::Io(format!("Corrupt archive: {e}")))?;
    let total = zip.len() as u64;
    std::fs::create_dir_all(dest).map_err(|e| CatableError::Io(e.to_string()))?;
    let mut last = Instant::now() - Duration::from_secs(1);

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| CatableError::Io(format!("Corrupt archive entry: {e}")))?;
        // Zip-slip guard: only accept paths that stay inside `dest`.
        let Some(rel): Option<PathBuf> = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        if skip_entry(&rel) {
            continue;
        }
        let out_path = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| CatableError::Io(e.to_string()))?;
        } else {
            if let Some(p) = out_path.parent() {
                std::fs::create_dir_all(p).map_err(|e| CatableError::Io(e.to_string()))?;
            }
            let f = std::fs::File::create(&out_path)
                .map_err(|e| CatableError::Io(format!("{}: {e}", out_path.display())))?;
            let mut w = BufWriter::with_capacity(256 * 1024, f);
            std::io::copy(&mut entry, &mut w).map_err(|e| CatableError::Io(e.to_string()))?;
            w.flush().map_err(|e| CatableError::Io(e.to_string()))?;
        }
        if last.elapsed() >= Duration::from_millis(150) {
            last = Instant::now();
            progress(ServiceProgress {
                phase: "extract",
                done: i as u64 + 1,
                total,
                message: "Extracting binaries".into(),
            });
        }
    }
    progress(ServiceProgress { phase: "extract", done: total, total, message: "Extraction complete".into() });
    Ok(())
}

/// Locate an executable (by file name, case-insensitive) anywhere ≤ 4 levels
/// deep under `root` — vendor archives nest binaries under varying folders.
pub fn find_exe(root: &Path, names: &[&str]) -> Option<PathBuf> {
    fn walk(dir: &Path, names: &[String], depth: u8) -> Option<PathBuf> {
        let entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
        for e in &entries {
            let p = e.path();
            if p.is_file() {
                let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                if names.iter().any(|x| *x == n) {
                    return Some(p);
                }
            }
        }
        if depth == 0 {
            return None;
        }
        // Prefer `bin` folders first for speed.
        let mut dirs: Vec<PathBuf> = entries.iter().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort_by_key(|p| !p.ends_with("bin"));
        dirs.into_iter().find_map(|d| walk(&d, names, depth - 1))
    }
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let names: Vec<String> = names.iter().map(|n| format!("{n}{suffix}").to_ascii_lowercase()).collect();
    walk(root, &names, 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_bulky_entries() {
        assert!(skip_entry(Path::new("pgsql/pgAdmin 4/bin/x.dll")));
        assert!(skip_entry(Path::new("pgsql/doc/postgresql/html/a.html")));
        assert!(skip_entry(Path::new("mysql-8.4.5-winx64/bin/mysqld.pdb")));
        assert!(!skip_entry(Path::new("pgsql/bin/postgres.exe")));
        assert!(!skip_entry(Path::new("mysql-8.4.5-winx64/lib/plugin/auth.dll")));
        assert!(!skip_entry(Path::new("mysql-8.4.5-winx64/share/english/errmsg.sys")));
    }
}
