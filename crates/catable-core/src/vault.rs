use crate::connection::{ConnectionConfig, DriverKind, Environment};
use crate::error::CataleError;
use crate::paths::app_data_dir;
use rusqlite::{params, Connection};
use std::sync::Mutex;

pub struct Vault {
    conn: Mutex<Connection>,
}

impl Vault {
    pub fn open() -> Result<Self, CataleError> {
        let dir = app_data_dir()?;
        let path = dir.join("vault.db");
        let conn = Connection::open(&path).map_err(|e| CataleError::Vault(e.to_string()))?;

        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| CataleError::Vault(e.to_string()))?;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS connections (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                driver      TEXT NOT NULL,
                environment TEXT NOT NULL,
                host        TEXT NOT NULL,
                port        INTEGER NOT NULL,
                database    TEXT NOT NULL,
                user        TEXT NOT NULL,
                read_only   INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .map_err(|e| CataleError::Vault(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn save_connection(&self, cfg: &ConnectionConfig) -> Result<(), CataleError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CataleError::Vault(e.to_string()))?;
        let driver_str = serde_json::to_string(&cfg.driver)
            .map_err(|e| CataleError::Serialization(e.to_string()))?;
        let env_str = serde_json::to_string(&cfg.environment)
            .map_err(|e| CataleError::Serialization(e.to_string()))?;

        conn.execute(
            r#"
            INSERT INTO connections (id, name, driver, environment, host, port, database, user, read_only)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                driver = excluded.driver,
                environment = excluded.environment,
                host = excluded.host,
                port = excluded.port,
                database = excluded.database,
                user = excluded.user,
                read_only = excluded.read_only;
            "#,
            params![
                cfg.id,
                cfg.name,
                driver_str.trim_matches('"'),
                env_str.trim_matches('"'),
                cfg.host,
                cfg.port,
                cfg.database,
                cfg.user,
                if cfg.read_only { 1 } else { 0 }
            ],
        )
        .map_err(|e| CataleError::Vault(e.to_string()))?;

        Ok(())
    }

    pub fn list_connections(&self) -> Result<Vec<ConnectionConfig>, CataleError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CataleError::Vault(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT id, name, driver, environment, host, port, database, user, read_only FROM connections ORDER BY name ASC")
            .map_err(|e| CataleError::Vault(e.to_string()))?;

        let rows = stmt
            .query_map([], |r| {
                let driver_s: String = r.get(2)?;
                let env_s: String = r.get(3)?;
                let read_only_int: i32 = r.get(8)?;

                let driver: DriverKind = serde_json::from_str(&format!("\"{}\"", driver_s))
                    .unwrap_or(DriverKind::Sqlite);
                let environment: Environment = serde_json::from_str(&format!("\"{}\"", env_s))
                    .unwrap_or(Environment::Development);

                Ok(ConnectionConfig {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    driver,
                    environment,
                    host: r.get(4)?,
                    port: r.get(5)?,
                    database: r.get(6)?,
                    user: r.get(7)?,
                    password: None,
                    read_only: read_only_int != 0,
                })
            })
            .map_err(|e| CataleError::Vault(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| CataleError::Vault(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn delete_connection(&self, id: &str) -> Result<(), CataleError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CataleError::Vault(e.to_string()))?;
        conn.execute("DELETE FROM connections WHERE id = ?1", params![id])
            .map_err(|e| CataleError::Vault(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_connection_persistence() {
        let tmp = std::env::temp_dir().join(format!("catable-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&tmp).unwrap();
        unsafe { std::env::set_var("CATABLE_DATA_DIR", &tmp) };

        let vault = Vault::open().unwrap();
        let cfg = ConnectionConfig {
            id: "conn-1".into(),
            name: "Local Postgres".into(),
            driver: DriverKind::Postgres,
            environment: Environment::Development,
            host: "127.0.0.1".into(),
            port: 5432,
            database: "app".into(),
            user: "postgres".into(),
            password: None,
            read_only: false,
        };

        vault.save_connection(&cfg).unwrap();
        let list = vault.list_connections().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Local Postgres");

        vault.delete_connection("conn-1").unwrap();
        assert_eq!(vault.list_connections().unwrap().len(), 0);
    }
}
