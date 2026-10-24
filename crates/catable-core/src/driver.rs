use crate::connection::{ConnectionConfig, DriverKind};
use crate::error::CatableError;
use crate::executor::{ConnectionTestResult, QueryResult, TableInfo, TableColumnInfo};
use crate::guard::GuardVerdict;
use std::collections::HashMap;
use std::path::PathBuf;
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

#[allow(dead_code)]
pub struct DriverRegistry;

impl DriverRegistry {
    /// Tests a database connection with a real TCP and protocol handshake.
    pub async fn test_connection(conn: &ConnectionConfig) -> ConnectionTestResult {
        test_connection(conn).await
    }

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

    // -------------------------------------------------------------------
    // DuckDB — embedded analytical engine on local .duckdb files
    // -------------------------------------------------------------------

    /// Resolves the DuckDB database file path from the connection config.
    /// If `database` is empty or points to a non-existent file, the path
    /// resolves to the app data directory so local .duckdb files get
    /// created on first use (sandbox.duckdb equivalent).
    pub fn resolve_duckdb_path(conn: &ConnectionConfig) -> Result<PathBuf, CatableError> {
        let dir = crate::paths::app_data_dir()?;
        if conn.database.is_empty() || conn.database == ":memory:" {
            Ok(dir.join("sandbox.duckdb"))
        } else {
            let path = PathBuf::from(&conn.database);
            if !path.exists() {
                let in_app = dir.join(&conn.database);
                Ok(in_app)
            } else {
                Ok(path)
            }
        }
    }

    /// Opens a DuckDB connection, honoring `read_only` mode.
    fn open_duckdb(conn: &ConnectionConfig) -> Result<duckdb::Connection, CatableError> {
        let path = Self::resolve_duckdb_path(conn)?;
        if !path.exists() {
            let _ = std::fs::File::create(&path).map_err(|e| CatableError::Io(e.to_string()))?;
        }

        let cfg = if conn.read_only {
            duckdb::Config::default()
                .access_mode(duckdb::AccessMode::ReadOnly)
                .map_err(|e| CatableError::Driver {
                    driver: "duckdb".into(),
                    message: e.to_string(),
                })?
        } else {
            duckdb::Config::default()
        };

        duckdb::Connection::open_with_flags(&path, cfg).map_err(|e| CatableError::Driver {
            driver: "duckdb".into(),
            message: e.to_string(),
        })
    }

    /// Lists tables and their columns for a DuckDB database file asynchronously.
    pub async fn list_tables_duckdb(conn: &ConnectionConfig) -> Result<Vec<TableInfo>, CatableError> {
        let db = Self::open_duckdb(conn)?;

        let mut stmt = db
            .prepare(
                "SELECT table_name, column_name, data_type, \
                 CASE WHEN column_default = 'nextval(' THEN true ELSE false END as is_pk, \
                 NOT is_nullable as not_null \
                 FROM information_schema.columns \
                 WHERE table_schema = 'main' \
                 ORDER BY table_name, ordinal_position",
            )
            .map_err(|e| CatableError::Driver {
                driver: "duckdb".into(),
                message: e.to_string(),
            })?;

        let mut rows = stmt.query([]).map_err(|e| CatableError::Driver {
            driver: "duckdb".into(),
            message: e.to_string(),
        })?;

        let mut raw_cols = Vec::new();
        while let Ok(Some(row)) = rows.next() {
            let tbl: String = row.get(0).unwrap_or_default();
            let col: String = row.get(1).unwrap_or_default();
            let dt: String = row.get(2).unwrap_or_default();
            let is_pk: bool = row.get(3).unwrap_or(false);
            let not_null: bool = row.get(4).unwrap_or(false);
            raw_cols.push((tbl, col, dt, is_pk, not_null));
        }

        let mut table_map: HashMap<String, TableInfo> = HashMap::new();

        for (table_name, column_name, data_type, is_pk, not_null) in raw_cols {
            let entry = table_map.entry(table_name.clone()).or_insert_with(|| TableInfo {
                name: table_name.clone(),
                table_type: "table".into(),
                columns: vec![],
                row_count: 0,
            });
            entry.columns.push(TableColumnInfo {
                name: column_name,
                data_type,
                is_pk,
                not_null,
            });
        }

        for info in table_map.values_mut() {
            let count_sql =
                format!("SELECT COUNT(*) FROM \"{}\"", info.name.replace('"', "\"\""));
            if let Ok(mut count_stmt) = db.prepare(&count_sql) {
                if let Ok(count) = count_stmt.query_row([], |r| r.get::<_, i64>(0)) {
                    info.row_count = count.max(0) as u64;
                }
            }
        }

        Ok(table_map.into_values().collect())
    }

    /// Public alias for DuckDB table listing matching query runner conventions.
    pub async fn list_tables_async(conn: &ConnectionConfig) -> Result<Vec<TableInfo>, CatableError> {
        Self::list_tables_duckdb(conn).await
    }

    /// Executes a SQL query against a DuckDB database file.
    pub async fn execute_query_duckdb(
        conn: &ConnectionConfig,
        sql: &str,
        verdict: GuardVerdict,
        start: Instant,
    ) -> QueryResult {
        let db = match Self::open_duckdb(conn) {
            Ok(db) => db,
            Err(e) => {
                return QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                    duration_ms: start.elapsed().as_millis() as u64,
                    error: Some(format!("Database connection failed: {}", e)),
                    guard_verdict: Some(verdict),
                };
            }
        };

        let trimmed = sql.trim();
        let upper = trimmed.to_uppercase();
        let is_select =
            upper.starts_with("SELECT") || upper.starts_with("WITH") || upper.starts_with("EXPLAIN");

        if is_select {
            let mut stmt = match db.prepare(trimmed) {
                Ok(s) => s,
                Err(e) => {
                    return QueryResult {
                        columns: vec![],
                        rows: vec![],
                        rows_affected: 0,
                        duration_ms: start.elapsed().as_millis() as u64,
                        error: Some(format!("SQL Syntax / Prepare Error: {}", e)),
                        guard_verdict: Some(verdict),
                    };
                }
            };

            let col_names: Vec<String> = stmt
                .schema()
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect();
            let col_count = col_names.len();

            let mut rows = match stmt.query([]) {
                Ok(r) => r,
                Err(e) => {
                    return QueryResult {
                        columns: col_names,
                        rows: vec![],
                        rows_affected: 0,
                        duration_ms: start.elapsed().as_millis() as u64,
                        error: Some(format!("Query execution failed: {}", e)),
                        guard_verdict: Some(verdict),
                    };
                }
            };

            let mut rows_out = Vec::new();
            while let Ok(Some(row)) = rows.next() {
                let mut row_vals = Vec::with_capacity(col_count);
                for i in 0..col_count {
                    let val = match row.get_ref(i) {
                        Ok(v) => duckdb_val_to_json(v),
                        Err(_) => serde_json::Value::Null,
                    };
                    row_vals.push(val);
                }
                rows_out.push(row_vals);

                if rows_out.len() >= 10_000 {
                    break;
                }
            }

            let count = rows_out.len() as u64;
            QueryResult {
                columns: col_names,
                rows: rows_out,
                rows_affected: count,
                duration_ms: start.elapsed().as_millis() as u64,
                error: None,
                guard_verdict: Some(verdict),
            }
        } else {
            // Write / DDL query
            if conn.read_only {
                return QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                    duration_ms: start.elapsed().as_millis() as u64,
                    error: Some("DuckDB connection is READ-ONLY; write queries rejected.".into()),
                    guard_verdict: Some(verdict),
                };
            }

            match db.execute(trimmed, []) {
                Ok(affected) => QueryResult {
                    columns: vec!["rows_affected".into()],
                    rows: vec![vec![serde_json::json!(affected)]],
                    rows_affected: affected as u64,
                    duration_ms: start.elapsed().as_millis() as u64,
                    error: None,
                    guard_verdict: Some(verdict),
                },
                Err(e) => QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                    duration_ms: start.elapsed().as_millis() as u64,
                    error: Some(format!("Execution failed: {}", e)),
                    guard_verdict: Some(verdict),
                },
            }
        }
    }

    /// Public alias for DuckDB query execution matching query runner conventions.
    pub async fn execute_query_async(
        conn: &ConnectionConfig,
        sql: &str,
    ) -> QueryResult {
        let start = Instant::now();
        let verdict = crate::guard::GuardrailEngine::evaluate(sql, conn);
        if verdict.risk == crate::guard::QueryRisk::Blocked {
            return QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: 0,
                duration_ms: start.elapsed().as_millis() as u64,
                error: Some(format!(
                    "Query Blocked by Guardrail: {}",
                    verdict.reason.clone().unwrap_or_default()
                )),
                guard_verdict: Some(verdict),
            };
        }
        Self::execute_query_duckdb(conn, sql, verdict, start).await
    }

    // -------------------------------------------------------------------
    // Redis — key-value browsing (GET/SET/HGET/HSET/HGETALL) + PING
    // -------------------------------------------------------------------

    fn redis_builder(conn: &ConnectionConfig) -> Result<redis_async::client::ConnectionBuilder, CatableError> {
        let mut b = redis_async::client::ConnectionBuilder::new(&conn.host, conn.effective_port())
            .map_err(|e| CatableError::Driver {
                driver: "redis".into(),
                message: e.to_string(),
            })?;
        if let Some(pw) = &conn.password {
            b.password(pw);
        }
        if !conn.user.is_empty() {
            b.username(&conn.user);
        }
        Ok(b)
    }

    async fn redis_paired(
        conn: &ConnectionConfig,
    ) -> Result<redis_async::client::PairedConnection, CatableError> {
        let b = Self::redis_builder(conn)?;
        b.paired_connect()
            .await
            .map_err(|e| CatableError::Driver {
                driver: "redis".into(),
                message: e.to_string(),
            })
    }

    /// Connects to a Redis server and performs a PING test.
    pub async fn test_redis_ping(conn: &ConnectionConfig) -> ConnectionTestResult {
        let start = Instant::now();
        let addr = format!("redis://{}:{}", conn.host, conn.effective_port());

        match Self::redis_paired(conn).await {
            Ok(client) => match client.send(redis_cmd(&["PING"])).await {
                Ok(_) => ConnectionTestResult {
                    success: true,
                    server_version: "Redis (PING OK)".into(),
                    latency_ms: start.elapsed().as_millis() as u64,
                    message: format!("Successfully connected to Redis at {}", addr),
                },
                Err(e) => ConnectionTestResult {
                    success: false,
                    server_version: "Unknown".into(),
                    latency_ms: start.elapsed().as_millis() as u64,
                    message: format!("Redis PING failed: {}", e),
                },
            },
            Err(e) => ConnectionTestResult {
                success: false,
                server_version: "Unknown".into(),
                latency_ms: start.elapsed().as_millis() as u64,
                message: format!("Redis connection failed: {}", e),
            },
        }
    }

    /// Lists Redis keys matching a glob pattern (KEYS command).
    pub async fn list_redis_keys(
        conn: &ConnectionConfig,
        pattern: &str,
    ) -> Result<Vec<String>, CatableError> {
        let client = Self::redis_paired(conn).await?;
        let resp = client
            .send(redis_cmd(&["KEYS", pattern]))
            .await
            .map_err(|e| CatableError::Driver {
                driver: "redis".into(),
                message: e.to_string(),
            })?;
        match resp {
            redis_async::resp::RespValue::Array(items) => {
                let keys = items
                    .into_iter()
                    .filter_map(|v| match v {
                        redis_async::resp::RespValue::BulkString(b) => {
                            Some(String::from_utf8_lossy(&b).into_owned())
                        }
                        _ => None,
                    })
                    .collect();
                Ok(keys)
            }
            _ => Ok(vec![]),
        }
    }

    /// Gets a single Redis key's string value (GET command).
    pub async fn get_redis_key(conn: &ConnectionConfig, key: &str) -> Result<Option<String>, CatableError> {
        let client = Self::redis_paired(conn).await?;
        let resp = client
            .send(redis_cmd(&["GET", key]))
            .await
            .map_err(|e| CatableError::Driver {
                driver: "redis".into(),
                message: e.to_string(),
            })?;
        match resp {
            redis_async::resp::RespValue::BulkString(b) => Ok(Some(String::from_utf8_lossy(&b).into_owned())),
            redis_async::resp::RespValue::Nil => Ok(None),
            _ => Ok(None),
        }
    }

    /// Sets a Redis key to a string value (SET command).
    pub async fn set_redis_key(conn: &ConnectionConfig, key: &str, value: &str) -> Result<(), CatableError> {
        let client = Self::redis_paired(conn).await?;
        client
            .send(redis_cmd(&["SET", key, value]))
            .await
            .map_err(|e| CatableError::Driver {
                driver: "redis".into(),
                message: e.to_string(),
            })?;
        Ok(())
    }

    /// Gets a single field from a Redis hash (HGET command).
    pub async fn hget_redis(
        conn: &ConnectionConfig,
        key: &str,
        field: &str,
    ) -> Result<Option<String>, CatableError> {
        let client = Self::redis_paired(conn).await?;
        let resp = client
            .send(redis_cmd(&["HGET", key, field]))
            .await
            .map_err(|e| CatableError::Driver {
                driver: "redis".into(),
                message: e.to_string(),
            })?;
        match resp {
            redis_async::resp::RespValue::BulkString(b) => Ok(Some(String::from_utf8_lossy(&b).into_owned())),
            redis_async::resp::RespValue::Nil => Ok(None),
            _ => Ok(None),
        }
    }

    /// Sets a field in a Redis hash to a value (HSET command).
    pub async fn hset_redis(
        conn: &ConnectionConfig,
        key: &str,
        field: &str,
        value: &str,
    ) -> Result<(), CatableError> {
        let client = Self::redis_paired(conn).await?;
        client
            .send(redis_cmd(&["HSET", key, field, value]))
            .await
            .map_err(|e| CatableError::Driver {
                driver: "redis".into(),
                message: e.to_string(),
            })?;
        Ok(())
    }

    /// Lists all fields and values of a Redis hash (HGETALL command).
    pub async fn hgetall_redis(
        conn: &ConnectionConfig,
        key: &str,
    ) -> Result<HashMap<String, String>, CatableError> {
        let client = Self::redis_paired(conn).await?;
        let resp = client
            .send(redis_cmd(&["HGETALL", key]))
            .await
            .map_err(|e| CatableError::Driver {
                driver: "redis".into(),
                message: e.to_string(),
            })?;
        let mut map = HashMap::new();
        if let redis_async::resp::RespValue::Array(items) = resp {
            let mut iter = items.into_iter();
            while let (Some(f), Some(v)) = (iter.next(), iter.next()) {
                if let (
                    redis_async::resp::RespValue::BulkString(fb),
                    redis_async::resp::RespValue::BulkString(vb),
                ) = (f, v)
                {
                    map.insert(
                        String::from_utf8_lossy(&fb).into_owned(),
                        String::from_utf8_lossy(&vb).into_owned(),
                    );
                }
            }
        }
        Ok(map)
    }
}

/// Helper converting a command string slice into a Redis RESP Array.
fn redis_cmd(args: &[&str]) -> redis_async::resp::RespValue {
    redis_async::resp::RespValue::Array(
        args.iter()
            .map(|s| redis_async::resp::RespValue::BulkString(s.as_bytes().to_vec()))
            .collect(),
    )
}

/// Helper converting a DuckDB ValueRef to serde_json::Value.
fn duckdb_val_to_json(v: duckdb::types::ValueRef) -> serde_json::Value {
    match v {
        duckdb::types::ValueRef::Null => serde_json::Value::Null,
        duckdb::types::ValueRef::Boolean(b) => serde_json::json!(b),
        duckdb::types::ValueRef::TinyInt(n) => serde_json::json!(n),
        duckdb::types::ValueRef::SmallInt(n) => serde_json::json!(n),
        duckdb::types::ValueRef::Int(n) => serde_json::json!(n),
        duckdb::types::ValueRef::BigInt(n) => serde_json::json!(n),
        duckdb::types::ValueRef::HugeInt(n) => serde_json::json!(n.to_string()),
        duckdb::types::ValueRef::UTinyInt(n) => serde_json::json!(n),
        duckdb::types::ValueRef::USmallInt(n) => serde_json::json!(n),
        duckdb::types::ValueRef::UInt(n) => serde_json::json!(n),
        duckdb::types::ValueRef::UBigInt(n) => serde_json::json!(n),
        duckdb::types::ValueRef::UHugeInt(n) => serde_json::json!(n.to_string()),
        duckdb::types::ValueRef::Float(f) => serde_json::json!(f),
        duckdb::types::ValueRef::Double(f) => serde_json::json!(f),
        duckdb::types::ValueRef::Decimal(d) => serde_json::json!(d.to_string()),
        duckdb::types::ValueRef::Timestamp(_, ts) => serde_json::json!(ts),
        duckdb::types::ValueRef::Text(bytes) => {
            serde_json::Value::String(String::from_utf8_lossy(bytes).into_owned())
        }
        duckdb::types::ValueRef::Blob(b) => {
            serde_json::Value::String(format!("<BLOB {} bytes>", b.len()))
        }
        _ => serde_json::Value::Null,
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
            match DriverRegistry::resolve_duckdb_path(conn) {
                Ok(path) => {
                    let cfg = if conn.read_only {
                        duckdb::Config::default()
                            .access_mode(duckdb::AccessMode::ReadOnly)
                            .ok()
                            .unwrap_or_default()
                    } else {
                        duckdb::Config::default()
                    };
                    match duckdb::Connection::open_with_flags(&path, cfg) {
                        Ok(_) => ConnectionTestResult {
                            success: true,
                            server_version: "DuckDB (local engine)".into(),
                            latency_ms: start.elapsed().as_millis() as u64,
                            message: format!(
                                "DuckDB local storage engine ready at {:?}",
                                path.file_name().unwrap_or_default()
                            ),
                        },
                        Err(e) => ConnectionTestResult {
                            success: false,
                            server_version: "Unknown".into(),
                            latency_ms: start.elapsed().as_millis() as u64,
                            message: format!("DuckDB open failed: {}", e),
                        },
                    }
                }
                Err(e) => ConnectionTestResult {
                    success: false,
                    server_version: "Unknown".into(),
                    latency_ms: start.elapsed().as_millis() as u64,
                    message: format!("DuckDB path resolution failed: {}", e),
                },
            }
        }
        DriverKind::Redis => DriverRegistry::test_redis_ping(conn).await,
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

    fn duckdb_conn(database: &str, read_only: bool) -> ConnectionConfig {
        ConnectionConfig {
            id: "duckdb-test".into(),
            name: "DuckDB Test".into(),
            driver: DriverKind::DuckDb,
            environment: Environment::Development,
            host: "".into(),
            port: 0,
            database: database.into(),
            user: "".into(),
            password: None,
            read_only,
        }
    }

    #[tokio::test]
    async fn duckdb_test_connection_and_table_listing() {
        let tmp = std::env::temp_dir().join(format!(
            "catable-duckdb-test-{}.duckdb",
            uuid::Uuid::now_v7()
        ));
        let conn = duckdb_conn(&tmp.to_string_lossy(), false);
        // Create table
        let verdict = crate::guard::GuardrailEngine::evaluate(
            "CREATE TABLE t1 (id INTEGER, name VARCHAR)",
            &conn,
        );
        let start = Instant::now();
        let res = DriverRegistry::execute_query_duckdb(
            &conn,
            "CREATE TABLE t1 (id INTEGER, name VARCHAR)",
            verdict,
            start,
        )
        .await;
        assert!(res.error.is_none(), "create table failed: {:?}", res.error);

        let tables = DriverRegistry::list_tables_duckdb(&conn)
            .await
            .expect("duckdb list_tables failed");
        assert!(tables.iter().any(|t| t.name == "t1"));

        let res = test_connection(&conn).await;
        assert!(res.success, "duckdb test_connection failed: {}", res.message);

        std::fs::remove_file(&tmp).ok();
    }

    #[tokio::test]
    async fn duckdb_execute_select_query() {
        let conn = duckdb_conn(":memory:", false);
        let verdict = crate::guard::GuardrailEngine::evaluate("SELECT 1 AS one", &conn);
        let start = std::time::Instant::now();
        let res =
            DriverRegistry::execute_query_duckdb(&conn, "SELECT 1 AS one", verdict, start).await;
        assert!(res.error.is_none(), "duckdb select failed: {:?}", res.error);
        assert_eq!(res.columns, vec!["one"]);
        assert_eq!(res.rows, vec![vec![serde_json::json!(1)]]);
    }

    #[tokio::test]
    async fn duckdb_read_only_blocks_writes() {
        let tmp = std::env::temp_dir().join(format!(
            "catable-duckdb-ro-{}.duckdb",
            uuid::Uuid::now_v7()
        ));
        // First create table with non-read-only
        {
            let conn_rw = duckdb_conn(&tmp.to_string_lossy(), false);
            let verdict = crate::guard::GuardrailEngine::evaluate("CREATE TABLE t (id INT)", &conn_rw);
            let start = Instant::now();
            let _ = DriverRegistry::execute_query_duckdb(&conn_rw, "CREATE TABLE t (id INT)", verdict, start).await;
        }

        // Open read-only
        let conn_ro = duckdb_conn(&tmp.to_string_lossy(), true);
        let verdict = crate::guard::GuardrailEngine::evaluate("SELECT count(*) FROM t", &conn_ro);
        let start = Instant::now();
        let res = DriverRegistry::execute_query_duckdb(&conn_ro, "SELECT count(*) FROM t", verdict, start).await;
        assert!(res.error.is_none(), "read query on read-only failed: {:?}", res.error);

        let verdict2 = crate::guard::GuardrailEngine::evaluate("INSERT INTO t VALUES (1)", &conn_ro);
        let start2 = Instant::now();
        let res2 = DriverRegistry::execute_query_duckdb(&conn_ro, "INSERT INTO t VALUES (1)", verdict2, start2).await;
        assert!(res2.error.is_some(), "write on read-only connection should fail");

        std::fs::remove_file(&tmp).ok();
    }

    #[tokio::test]
    async fn redis_ping_fails_gracefully_without_server() {
        let conn = ConnectionConfig {
            id: "redis-test".into(),
            name: "Redis Test".into(),
            driver: DriverKind::Redis,
            environment: Environment::Development,
            host: "127.0.0.1".into(),
            port: 59998,
            database: "".into(),
            user: "".into(),
            password: None,
            read_only: false,
        };
        let res = DriverRegistry::test_redis_ping(&conn).await;
        assert!(!res.success, "expected redis ping to fail: {}", res.message);
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
