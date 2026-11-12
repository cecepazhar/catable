// DB Service Engine catalog and state
use crate::error::CatableError;
use crate::paths::app_data_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineType {
    Postgres,
    Mysql,
    Mariadb,
}

impl std::fmt::Display for EngineType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineType::Postgres => write!(f, "PostgreSQL"),
            EngineType::Mysql => write!(f, "MySQL"),
            EngineType::Mariadb => write!(f, "MariaDB"),
        }
    }
}

/// Available engine versions per engine, in descending order (newest first).
/// Used by the UI "New Service" form and by the auto-downloader to build
/// the correct binary URL.
pub const AVAILABLE_VERSIONS: &[(EngineType, &[&str])] = &[
    (EngineType::Postgres, &["17.5", "16.9", "15.10"]),
    (EngineType::Mysql, &["8.4", "8.0", "5.7"]),
    (EngineType::Mariadb, &["11.8", "11.2", "10.11"]),
];

impl EngineType {
    pub fn versions(&self) -> &'static [&'static str] {
        for (engine, versions) in AVAILABLE_VERSIONS {
            if *engine == *self {
                return versions;
            }
        }
        &[]
    }

    pub fn default_port(&self) -> u16 {
        match self {
            EngineType::Postgres => 5432,
            EngineType::Mysql | EngineType::Mariadb => 3306,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failing,
}

impl Default for ServiceState {
    fn default() -> Self {
        Self::Stopped
    }
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
    pub bin_dir: PathBuf,
    #[serde(default)]
    pub state: ServiceState,
    pub created_at: i64,
}

fn services_base_dir() -> Result<PathBuf, CatableError> {
    let dir = app_data_dir()?;
    let base = dir.join("db-engines");
    std::fs::create_dir_all(&base).map_err(|e| CatableError::Io(e.to_string()))?;
    Ok(base)
}

/// In-memory catalog of all local DB service instances managed by CATable.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ServiceCatalog {
    pub services: Vec<DbService>,
}

impl ServiceCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_from_disk() -> Self {
        let dir = match services_base_dir() {
            Ok(d) => d,
            Err(_) => return Self::default(),
        };
        let path = dir.join("catalog.json");
        if let Ok(data) = std::fs::read_to_string(&path) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) -> Result<(), CatableError> {
        let dir = services_base_dir()?;
        let path = dir.join("catalog.json");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| CatableError::Serialization(e.to_string()))?;
        std::fs::write(path, json).map_err(|e| CatableError::Io(e.to_string()))?;
        Ok(())
    }

    /// Find first free port starting from `preferred`, avoiding
    /// collisions with running/stopped services in the catalog.
    pub fn free_port(&self, preferred: u16) -> u16 {
        for p in preferred..(preferred + 50) {
            if !self
                .services
                .iter()
                .any(|s| s.port == p && s.state != ServiceState::Stopped)
            {
                return p;
            }
        }
        preferred + 100
    }

    /// Create a new service definition (not yet running).
    pub fn add_service(
        &mut self,
        engine: EngineType,
        version: &str,
        name: &str,
        preferred_port: u16,
    ) -> Result<DbService, CatableError> {
        let port = self.free_port(preferred_port);
        let base = services_base_dir()?;
        let engine_dir = base.join(format!("{}-{}", engine, version));
        let data_dir = engine_dir
            .join(format!("data-{}", name.replace(' ', "-").to_lowercase()));
        let bin_dir = engine_dir.join("bin");

        let svc = DbService {
            id: Uuid::new_v4().to_string(),
            engine,
            version: version.to_string(),
            name: name.to_string(),
            port,
            data_dir: data_dir.clone(),
            bin_dir: bin_dir.clone(),
            state: ServiceState::Stopped,
            created_at: chrono::Utc::now().timestamp(),
        };

        std::fs::create_dir_all(&data_dir).map_err(|e| CatableError::Io(e.to_string()))?;
        std::fs::create_dir_all(&bin_dir).map_err(|e| CatableError::Io(e.to_string()))?;

        self.services.push(svc.clone());
        self.save()?;
        Ok(svc)
    }

    pub fn get(&self, id: &str) -> Option<&DbService> {
        self.services.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut DbService> {
        self.services.iter_mut().find(|s| s.id == id)
    }

    pub fn remove(&mut self, id: &str) -> Option<DbService> {
        let idx = self.services.iter().position(|s| s.id == id)?;
        let svc = self.services.remove(idx);
        let _ = self.save();
        Some(svc)
    }

    pub fn update_state(&mut self, id: &str, state: ServiceState) {
        if let Some(s) = self.get_mut(id) {
            s.state = state;
            let _ = self.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_port_avoids_running_services() {
        let mut cat = ServiceCatalog::new();
        let s1 = cat.add_service(EngineType::Postgres, "16", "svc-a", 5432).unwrap();
        assert_eq!(s1.port, 5432);

        cat.update_state(&s1.id, ServiceState::Running);
        let next = cat.free_port(5432);
        assert_eq!(next, 5433);
    }

    #[test]
    fn add_and_get_service() {
        let mut cat = ServiceCatalog::new();
        let svc = cat.add_service(EngineType::Mysql, "8.0", "svc-b", 3306).unwrap();
        assert_eq!(svc.engine, EngineType::Mysql);
        assert_eq!(cat.get(&svc.id).unwrap().name, "svc-b");
    }

    #[test]
    fn remove_service() {
        let mut cat = ServiceCatalog::new();
        let svc = cat.add_service(EngineType::Mariadb, "11", "svc-c", 3306).unwrap();
        let removed = cat.remove(&svc.id);
        assert!(removed.is_some());
        assert!(cat.get(&svc.id).is_none());
    }
}
