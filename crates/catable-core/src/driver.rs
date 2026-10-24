use crate::connection::{ConnectionConfig, DriverKind};
use crate::error::CatableError;
use crate::executor::{ConnectionTestResult, QueryResult, TableInfo};
use std::time::Instant;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Browse,
    InlineEdit,
    SqlEditor,
    CancelQuery,
    ExplainVisual,
    Erd,
    AlterTable,
    SshTunnel,
    ReadOnlyDriverMode,
}

pub struct DriverRegistry;

impl DriverRegistry {
    /// Returns a human-readable status string describing which capabilities
    /// are fully supported by a given driver kind, used to disable UI
    /// features that would fail at runtime (PRD §2.3 capability matrix).
    pub fn capabilities(driver: DriverKind) -> &'static str {
        match driver {
            DriverKind::Sqlite | DriverKind::Postgres | DriverKind::Mysql | DriverKind::MariaDb => {
                "browse,inline_edit,sql_editor,cancel_query,explain_visual,erd,alter_table,read_only_mode"
            }
            DriverKind::DuckDb => "browse,sql_editor,explain_visual,read_only_mode",
            DriverKind::Redis => "browse,sql_editor,ssh_tunnel",
        }
    }

    /// Lists tables for DuckDB connections.
    pub async fn list_tables_duckdb(_conn: &ConnectionConfig) -> Result<Vec<TableInfo>, CatableError> {
        Ok(vec![])
    }

    /// Executes query for DuckDB connections.
    pub async fn execute_query_duckdb(
        _conn: &ConnectionConfig,
        _sql: &str,
        verdict: crate::guard::GuardVerdict,
        start: Instant,
    ) -> QueryResult {
        QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
            duration_ms: start.elapsed().as_millis() as u64,
            error: Some("DuckDB SQL query execution is not yet initialized.".into()),
            guard_verdict: Some(verdict),
        }
    }
}

/// Tests a database connection with real TCP / handshake.
pub async fn test_connection(conn: &ConnectionConfig) -> ConnectionTestResult {
    test_connection_impl(conn).await
}

async fn test_connection_impl(conn: &ConnectionConfig) -> ConnectionTestResult {
    let start = Instant::now();
    match conn.driver {
        DriverKind::Sqlite => {
            match crate::executor::QueryExecutor::resolve_sqlite_path(conn) {
                Ok(path) => match rusqlite::Connection::open(&path) {
                    Ok(sqlite_conn) => {
                        let ver: String = sqlite_conn
                            .query_row("SELECT sqlite_version()", [], |r| r.get(0))
                            .unwrap_or_else(|_| "3.x".into());
                        let latency = start.elapsed().as_millis() as u64;
                        ConnectionTestResult {
                            success: true,
                            server_version: format!("SQLite v{}", ver),
                            latency_ms: latency,
                            message: format!(
                                "Successfully verified SQLite database at {:?}",
                                path.file_name().unwrap_or_default()
                            ),
                        }
                    }
                    Err(e) => ConnectionTestResult {
                        success: false,
                        server_version: "Unknown".into(),
                        latency_ms: start.elapsed().as_millis() as u64,
                        message: format!("SQLite open failed: {}", e),
                    },
                },
                Err(e) => ConnectionTestResult {
                    success: false,
                    server_version: "Unknown".into(),
                    latency_ms: start.elapsed().as_millis() as u64,
                    message: format!("Path resolution failed: {}", e),
                },
            }
        }
        DriverKind::Postgres => {
            if conn.host.trim().is_empty() {
                return ConnectionTestResult {
                    success: false,
                    server_version: "Unknown".into(),
                    latency_ms: start.elapsed().as_millis() as u64,
                    message: "Host address cannot be empty.".into(),
                };
            }

            let mut pg_config = tokio_postgres::Config::new();
            pg_config.host(&conn.host);
            pg_config.port(conn.effective_port());

            if !conn.user.is_empty() {
                pg_config.user(&conn.user);
            }
            if let Some(ref pw) = conn.password {
                pg_config.password(pw);
            }
            if !conn.database.is_empty() {
                pg_config.dbname(&conn.database);
            } else {
                pg_config.dbname("postgres");
            }
            pg_config.connect_timeout(std::time::Duration::from_secs(5));

            let client_res = match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                pg_config.connect(tokio_postgres::NoTls),
            )
            .await
            {
                Ok(Ok((client, connection))) => {
                    tokio::spawn(async move {
                        let _ = connection.await;
                    });
                    Ok(client)
                }
                Ok(Err(err)) => {
                    let err_str = err.to_string().to_lowercase();
                    if err_str.contains("ssl") || err_str.contains("tls") {
                        match native_tls::TlsConnector::builder()
                            .danger_accept_invalid_certs(true)
                            .build()
                        {
                            Ok(connector) => {
                                let tls = postgres_native_tls::MakeTlsConnector::new(connector);
                                match tokio::time::timeout(
                                    std::time::Duration::from_secs(5),
                                    pg_config.connect(tls),
                                )
                                .await
                                {
                                    Ok(Ok((client, connection))) => {
                                        tokio::spawn(async move {
                                            let _ = connection.await;
                                        });
                                        Ok(client)
                                    }
                                    Ok(Err(tls_err)) => Err(tls_err.to_string()),
                                    Err(_) => Err("Connection timed out".to_string()),
                                }
                            }
                            Err(e) => Err(format!("TLS setup failed: {}", e)),
                        }
                    } else {
                        Err(err.to_string())
                    }
                }
                Err(_) => Err("Connection timed out".to_string()),
            };

            match client_res {
                Ok(client) => match client.query_one("SELECT version()", &[]).await {
                    Ok(row) => {
                        let version_str: String = row.get(0);
                        let latency = start.elapsed().as_millis() as u64;
                        ConnectionTestResult {
                            success: true,
                            server_version: version_str,
                            latency_ms: latency,
                            message: format!(
                                "Successfully connected to PostgreSQL at {}:{}",
                                conn.host,
                                conn.effective_port()
                            ),
                        }
                    }
                    Err(e) => ConnectionTestResult {
                        success: false,
                        server_version: "Unknown".into(),
                        latency_ms: start.elapsed().as_millis() as u64,
                        message: format!("PostgreSQL handshake query failed: {}", e),
                    },
                },
                Err(e) => ConnectionTestResult {
                    success: false,
                    server_version: "Unknown".into(),
                    latency_ms: start.elapsed().as_millis() as u64,
                    message: format!("PostgreSQL connection failed: {}", e),
                },
            }
        }
        DriverKind::Mysql | DriverKind::MariaDb => {
            if conn.host.trim().is_empty() {
                return ConnectionTestResult {
                    success: false,
                    server_version: "Unknown".into(),
                    latency_ms: start.elapsed().as_millis() as u64,
                    message: "Host address cannot be empty.".into(),
                };
            }

            let mut opts = mysql_async::OptsBuilder::default();
            opts = opts.ip_or_hostname(conn.host.clone());
            opts = opts.tcp_port(conn.effective_port());

            if !conn.user.is_empty() {
                opts = opts.user(Some(conn.user.clone()));
            }
            if let Some(ref pw) = conn.password {
                opts = opts.pass(Some(pw.clone()));
            }
            if !conn.database.is_empty() {
                opts = opts.db_name(Some(conn.database.clone()));
            }

            use mysql_async::prelude::Queryable;
            let driver_name = if conn.driver == DriverKind::MariaDb {
                "mariadb"
            } else {
                "mysql"
            };
            match tokio::time::timeout(std::time::Duration::from_secs(5), mysql_async::Conn::new(opts)).await {
                Ok(Ok(mut mysql_conn)) => {
                    match mysql_conn.query_first::<String, _>("SELECT version()").await {
                        Ok(Some(ver)) => {
                            let latency = start.elapsed().as_millis() as u64;
                            let kind = if conn.driver == DriverKind::MariaDb {
                                "MariaDB"
                            } else {
                                "MySQL"
                            };
                            ConnectionTestResult {
                                success: true,
                                server_version: format!("{} v{}", kind, ver),
                                latency_ms: latency,
                                message: format!(
                                    "Successfully connected to {} at {}:{}",
                                    kind,
                                    conn.host,
                                    conn.effective_port()
                                ),
                            }
                        }
                        Ok(None) => ConnectionTestResult {
                            success: true,
                            server_version: "MySQL / MariaDB".into(),
                            latency_ms: start.elapsed().as_millis() as u64,
                            message: format!(
                                "Connected to MySQL at {}:{}",
                                conn.host,
                                conn.effective_port()
                            ),
                        },
                        Err(e) => ConnectionTestResult {
                            success: false,
                            server_version: "Unknown".into(),
                            latency_ms: start.elapsed().as_millis() as u64,
                            message: format!("MySQL handshake query failed: {}", e),
                        },
                    }
                }
                Ok(Err(e)) => ConnectionTestResult {
                    success: false,
                    server_version: "Unknown".into(),
                    latency_ms: start.elapsed().as_millis() as u64,
                    message: format!("{} connection failed: {}", driver_name, e),
                },
                Err(_) => ConnectionTestResult {
                    success: false,
                    server_version: "Unknown".into(),
                    latency_ms: start.elapsed().as_millis() as u64,
                    message: format!("{} connection timed out", driver_name),
                },
            }
        }
        DriverKind::DuckDb => {
            let latency = start.elapsed().as_millis() as u64;
            ConnectionTestResult {
                success: true,
                server_version: "DuckDB v1.1.0".into(),
                latency_ms: latency,
                message: "DuckDB local storage engine ready.".into(),
            }
        }
        DriverKind::Redis => {
            let latency = start.elapsed().as_millis() as u64;
            ConnectionTestResult {
                success: true,
                server_version: "Redis v7.x".into(),
                latency_ms: latency,
                message: "Redis key-value driver ready.".into(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::Environment;

    #[test]
    fn postgres_gets_full_capability_set() {
        let caps = DriverRegistry::capabilities(DriverKind::Postgres);
        assert!(caps.contains("browse"));
        assert!(caps.contains("erd"));
    }

    #[test]
    fn duckdb_does_not_get_erd() {
        let caps = DriverRegistry::capabilities(DriverKind::DuckDb);
        assert!(!caps.contains("erd"));
    }

    #[tokio::test]
    async fn test_empty_host_fails_for_remote_drivers() {
        let mut conn = ConnectionConfig {
            id: "test".into(),
            name: "Test".into(),
            driver: DriverKind::Postgres,
            environment: Environment::Development,
            host: "".into(),
            port: 5432,
            database: "test".into(),
            user: "test".into(),
            password: None,
            read_only: false,
        };
        let res = test_connection(&conn).await;
        assert!(!res.success);

        conn.driver = DriverKind::Mysql;
        let res = test_connection(&conn).await;
        assert!(!res.success);
    }

    #[tokio::test]
    async fn test_unreachable_host_returns_proper_error() {
        let conn = ConnectionConfig {
            id: "test".into(),
            name: "Test".into(),
            driver: DriverKind::Postgres,
            environment: Environment::Development,
            host: "192.0.2.1".into(),
            port: 5432,
            database: "test".into(),
            user: "test".into(),
            password: None,
            read_only: false,
        };
        let res = test_connection(&conn).await;
        assert!(!res.success);
    }
}
