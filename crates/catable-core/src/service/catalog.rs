// DB Service Engine catalog: engine releases + persisted service instances.
use crate::error::CatableError;
use crate::paths::app_data_dir;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineType {
    Postgres,
    Mysql,
    Mariadb,
    Redis,
}

impl std::fmt::Display for EngineType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EngineType::Postgres => "PostgreSQL",
            EngineType::Mysql => "MySQL",
            EngineType::Mariadb => "MariaDB",
            EngineType::Redis => "Redis",
        })
    }
}

impl EngineType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "postgres" | "postgresql" | "pg" => Some(Self::Postgres),
            "mysql" => Some(Self::Mysql),
            "mariadb" => Some(Self::Mariadb),
            "redis" => Some(Self::Redis),
            _ => None,
        }
    }

    pub fn slug(&self) -> &'static str {
        match self {
            EngineType::Postgres => "postgres",
            EngineType::Mysql => "mysql",
            EngineType::Mariadb => "mariadb",
            EngineType::Redis => "redis",
        }
    }

    pub fn default_port(&self) -> u16 {
        match self {
            EngineType::Postgres => 5432,
            EngineType::Mysql | EngineType::Mariadb => 3306,
            EngineType::Redis => 6379,
        }
    }

    /// Versions available for this engine, newest first.
    pub fn versions(&self) -> Vec<&'static str> {
        RELEASES
            .iter()
            .filter(|r| r.engine == *self)
            .map(|r| r.version)
            .collect()
    }
}

/// A downloadable portable engine build.
#[derive(Debug, Clone, Copy)]
pub struct EngineRelease {
    pub engine: EngineType,
    pub version: &'static str,
    pub url: &'static str,
    /// Approximate archive size (bytes) — used for UI hints when the server
    /// does not send a Content-Length.
    pub size_bytes: u64,
}

/// Official portable Windows x64 builds (verified reachable, HTTPS only).
pub const RELEASES: &[EngineRelease] = &[
    EngineRelease { engine: EngineType::Postgres, version: "18.1", url: "https://get.enterprisedb.com/postgresql/postgresql-18.1-1-windows-x64-binaries.zip", size_bytes: 334_929_717 },
    EngineRelease { engine: EngineType::Postgres, version: "17.5", url: "https://get.enterprisedb.com/postgresql/postgresql-17.5-1-windows-x64-binaries.zip", size_bytes: 321_800_244 },
    EngineRelease { engine: EngineType::Postgres, version: "16.9", url: "https://get.enterprisedb.com/postgresql/postgresql-16.9-1-windows-x64-binaries.zip", size_bytes: 314_500_604 },
    EngineRelease { engine: EngineType::Postgres, version: "15.13", url: "https://get.enterprisedb.com/postgresql/postgresql-15.13-1-windows-x64-binaries.zip", size_bytes: 312_443_312 },
    EngineRelease { engine: EngineType::Mysql, version: "8.4.5", url: "https://cdn.mysql.com/archives/mysql-8.4/mysql-8.4.5-winx64.zip", size_bytes: 259_799_354 },
    EngineRelease { engine: EngineType::Mysql, version: "8.0.42", url: "https://cdn.mysql.com/archives/mysql-8.0/mysql-8.0.42-winx64.zip", size_bytes: 244_862_139 },
    EngineRelease { engine: EngineType::Mariadb, version: "11.8.3", url: "https://archive.mariadb.org/mariadb-11.8.3/winx64-packages/mariadb-11.8.3-winx64.zip", size_bytes: 93_931_568 },
    EngineRelease { engine: EngineType::Mariadb, version: "11.4.5", url: "https://archive.mariadb.org/mariadb-11.4.5/winx64-packages/mariadb-11.4.5-winx64.zip", size_bytes: 90_835_771 },
    EngineRelease { engine: EngineType::Mariadb, version: "10.11.11", url: "https://archive.mariadb.org/mariadb-10.11.11/winx64-packages/mariadb-10.11.11-winx64.zip", size_bytes: 90_002_455 },
    EngineRelease { engine: EngineType::Redis, version: "5.0.14", url: "https://github.com/tporadowski/redis/releases/download/v5.0.14.1/Redis-x64-5.0.14.1.zip", size_bytes: 12_617_669 },
];

pub fn find_release(engine: EngineType, version: &str) -> Option<&'static EngineRelease> {
    RELEASES
        .iter()
        .find(|r| r.engine == engine && r.version == version)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
    Failing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbService {
    pub id: String,
    pub engine: EngineType,
    pub version: String,
    pub name: String,
    pub port: u16,
    pub data_dir: PathBuf,
    /// Shared install directory of the engine binaries (one per engine+version).
    pub bin_dir: PathBuf,
    #[serde(default)]
    pub state: ServiceState,
    pub created_at: i64,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub auto_start: bool,
    /// Default database created on first start (empty for Redis).
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl DbService {
    pub fn default_user(&self) -> &'static str {
        match self.engine {
            EngineType::Postgres => "postgres",
            EngineType::Mysql | EngineType::Mariadb => "root",
            EngineType::Redis => "",
        }
    }
}

pub(crate) fn services_base_dir() -> Result<PathBuf, CatableError> {
    let base = app_data_dir()?.join("db-engines");
    std::fs::create_dir_all(&base).map_err(|e| CatableError::Io(e.to_string()))?;
    Ok(base)
}

pub(crate) fn engine_install_dir(engine: EngineType, version: &str) -> Result<PathBuf, CatableError> {
    Ok(services_base_dir()?
        .join("engines")
        .join(format!("{}-{}", engine.slug(), version)))
}

pub(crate) fn log_path(id: &str) -> Result<PathBuf, CatableError> {
    let dir = services_base_dir()?.join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| CatableError::Io(e.to_string()))?;
    Ok(dir.join(format!("{id}.log")))
}

/// Turn a free-form service name into a safe database identifier.
pub fn sanitize_db_name(name: &str) -> String {
    let mut out: String = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').to_string();
    let out = if out.is_empty() { "app".to_string() } else { out };
    if out.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        format!("db_{out}")
    } else {
        out.chars().take(48).collect()
    }
}

/// Serializes every read-modify-write of catalog.json.
static CATALOG_LOCK: Mutex<()> = parking_lot::const_mutex(());

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ServiceCatalog {
    pub services: Vec<DbService>,
}

impl ServiceCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    fn path() -> Result<PathBuf, CatableError> {
        Ok(services_base_dir()?.join("catalog.json"))
    }

    pub fn load_from_disk() -> Self {
        let _g = CATALOG_LOCK.lock();
        Self::load_unlocked()
    }

    fn load_unlocked() -> Self {
        match Self::path().and_then(|p| std::fs::read_to_string(p).map_err(|e| CatableError::Io(e.to_string()))) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    fn save_unlocked(&self) -> Result<(), CatableError> {
        let path = Self::path()?;
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| CatableError::Serialization(e.to_string()))?;
        std::fs::write(&tmp, json).map_err(|e| CatableError::Io(e.to_string()))?;
        std::fs::rename(&tmp, &path).map_err(|e| CatableError::Io(e.to_string()))?;
        Ok(())
    }

    /// Atomically load → mutate → save the on-disk catalog.
    pub fn update<R>(
        f: impl FnOnce(&mut ServiceCatalog) -> Result<R, CatableError>,
    ) -> Result<R, CatableError> {
        let _g = CATALOG_LOCK.lock();
        let mut cat = Self::load_unlocked();
        let r = f(&mut cat)?;
        cat.save_unlocked()?;
        Ok(r)
    }

    pub fn port_taken(&self, port: u16, except_id: Option<&str>) -> bool {
        self.services
            .iter()
            .any(|s| s.port == port && Some(s.id.as_str()) != except_id)
    }

    /// First port ≥ `preferred` not claimed by another catalog entry.
    pub fn free_port(&self, preferred: u16) -> u16 {
        let mut p = preferred;
        while self.port_taken(p, None) && p < u16::MAX {
            p += 1;
        }
        p
    }

    pub fn build_service(
        &self,
        engine: EngineType,
        version: &str,
        name: &str,
        port: u16,
        auto_start: bool,
    ) -> Result<DbService, CatableError> {
        let id = Uuid::new_v4().to_string();
        let data_dir = services_base_dir()?.join("data").join(&id);
        let bin_dir = engine_install_dir(engine, version)?;
        Ok(DbService {
            database: if engine == EngineType::Redis { String::new() } else { sanitize_db_name(name) },
            id,
            engine,
            version: version.to_string(),
            name: name.to_string(),
            port,
            data_dir,
            bin_dir,
            state: ServiceState::Stopped,
            created_at: chrono::Utc::now().timestamp(),
            pid: None,
            auto_start,
            last_error: None,
        })
    }

    pub fn get(&self, id: &str) -> Option<&DbService> {
        self.services.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut DbService> {
        self.services.iter_mut().find(|s| s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_has_a_release() {
        for e in [EngineType::Postgres, EngineType::Mysql, EngineType::Mariadb, EngineType::Redis] {
            assert!(!e.versions().is_empty(), "{e} has no release");
            assert!(find_release(e, e.versions()[0]).is_some());
        }
    }

    #[test]
    fn releases_are_https() {
        assert!(RELEASES.iter().all(|r| r.url.starts_with("https://")));
    }

    #[test]
    fn sanitize_names() {
        assert_eq!(sanitize_db_name("My Shop-DB"), "my_shop_db");
        assert_eq!(sanitize_db_name("  "), "app");
        assert_eq!(sanitize_db_name("2024 data"), "db_2024_data");
    }

    #[test]
    fn free_port_skips_catalog_ports() {
        let mut cat = ServiceCatalog::new();
        let mut a = cat.build_service(EngineType::Postgres, "17.5", "a", 5432, false).unwrap();
        a.port = 5432;
        cat.services.push(a);
        assert_eq!(cat.free_port(5432), 5433);
        assert_eq!(cat.free_port(5500), 5500);
    }
}
