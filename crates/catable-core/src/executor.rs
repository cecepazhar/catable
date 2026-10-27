use crate::connection::{ConnectionConfig, DriverKind};
use crate::error::CatableError;
use crate::guard::{GuardVerdict, GuardrailEngine, QueryRisk};
use crate::paths::app_data_dir;
use rusqlite::{params_from_iter, types::ValueRef, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableColumnInfo {
    pub name: String,
    pub data_type: String,
    pub is_pk: bool,
    pub not_null: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    pub name: String,
    pub table_type: String,
    pub columns: Vec<TableColumnInfo>,
    pub row_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub rows_affected: u64,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub guard_verdict: Option<GuardVerdict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTestResult {
    pub success: bool,
    pub server_version: String,
    pub latency_ms: u64,
    pub message: String,
}

pub struct QueryExecutor;

impl QueryExecutor {
    /// Resolves the sqlite path for a connection config.
    /// If empty or ":memory:", points to the app's default sandbox.db.
    pub fn resolve_sqlite_path(conn: &ConnectionConfig) -> Result<PathBuf, CatableError> {
        let dir = app_data_dir()?;
        if conn.database.is_empty() || conn.database == ":memory:" || conn.database == "sandbox" {
            let sandbox_path = dir.join("sandbox.db");
            Self::ensure_sample_data(&sandbox_path)?;
            Ok(sandbox_path)
        } else {
            let path = PathBuf::from(&conn.database);
            if !path.exists() {
                // If it's a relative name, place inside app data dir
                let in_app_dir = dir.join(&conn.database);
                if !in_app_dir.exists() {
                    Self::ensure_sample_data(&in_app_dir)?;
                }
                Ok(in_app_dir)
            } else {
                Ok(path)
            }
        }
    }

    /// Seeds the sandbox database with sample e-commerce data if empty.
    pub fn ensure_sample_data(path: &PathBuf) -> Result<(), CatableError> {
        let conn = Connection::open(path).map_err(|e| CatableError::Vault(e.to_string()))?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                email TEXT UNIQUE NOT NULL,
                role TEXT NOT NULL DEFAULT 'developer',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS products (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sku TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                category TEXT NOT NULL,
                price REAL NOT NULL,
                stock INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS orders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL REFERENCES users(id),
                total_amount REAL NOT NULL,
                status TEXT NOT NULL DEFAULT 'completed',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS order_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                order_id INTEGER NOT NULL REFERENCES orders(id),
                product_id INTEGER NOT NULL REFERENCES products(id),
                quantity INTEGER NOT NULL,
                unit_price REAL NOT NULL
            );
            "#,
        )
        .map_err(|e| CatableError::Vault(e.to_string()))?;

        // Seed if users table is empty
        let count: i64 = conn
            .query_row("SELECT count(*) FROM users", [], |r| r.get(0))
            .unwrap_or(0);

        if count == 0 {
            conn.execute_batch(
                r#"
                INSERT INTO users (name, email, role) VALUES
                    ('Cecep Saeful Azhar', 'cecep@fathforce.com', 'admin'),
                    ('Harun Al-Rasyid', 'harun@fathforce.com', 'dba'),
                    ('Fatih Al-Ayyubi', 'fatih@fathforce.com', 'developer'),
                    ('Ibrahim Saeful', 'ibrahim@fathforce.com', 'analyst'),
                    ('Rini Hidayat', 'rini@fathforce.com', 'designer');

                INSERT INTO products (sku, name, category, price, stock) VALUES
                    ('PRD-CAT-001', 'CATable Pro Lifetime License', 'Software', 99.00, 999),
                    ('PRD-CTB-002', 'CATable Studio Enterprise', 'Software', 149.00, 500),
                    ('PRD-CBK-003', 'CABook Publisher Suite', 'Software', 79.00, 750),
                    ('PRD-CLM-004', 'CACalm Focus Ambient Pack', 'Audio', 19.00, 1000),
                    ('PRD-SRV-005', 'High-Speed Dedicated Proxy Node', 'Infrastructure', 249.00, 50);

                INSERT INTO orders (user_id, total_amount, status) VALUES
                    (1, 248.00, 'completed'),
                    (2, 99.00, 'completed'),
                    (3, 149.00, 'processing'),
                    (4, 19.00, 'completed'),
                    (5, 79.00, 'refunded');

                INSERT INTO order_items (order_id, product_id, quantity, unit_price) VALUES
                    (1, 1, 1, 99.00),
                    (1, 2, 1, 149.00),
                    (2, 1, 1, 99.00),
                    (3, 2, 1, 149.00),
                    (4, 4, 1, 19.00),
                    (5, 3, 1, 79.00);
                "#,
            )
            .map_err(|e| CatableError::Vault(e.to_string()))?;
        }

        Ok(())
    }

    /// Tests a database connection asynchronously.
    pub async fn test_connection_async(conn: &ConnectionConfig) -> ConnectionTestResult {
        crate::driver::test_connection(conn).await
    }

    /// Tests a database connection synchronously.
    pub fn test_connection(conn: &ConnectionConfig) -> ConnectionTestResult {
        let conn_clone = conn.clone();
        run_async(async move { Self::test_connection_async(&conn_clone).await })
    }

    /// Lists tables and columns for the connection asynchronously.
    pub async fn list_tables_async(conn: &ConnectionConfig) -> Result<Vec<TableInfo>, CatableError> {
        let (effective_conn, _tunnel) = if conn.ssh_tunnel.as_ref().map_or(false, |t| t.enabled) {
            match crate::tunnel::open_tunnel(conn) {
                Ok(tun) => {
                    let mut c = conn.clone();
                    c.host = "127.0.0.1".into();
                    c.port = tun.local_port();
                    (c, Some(tun))
                }
                Err(e) => return Err(e),
            }
        } else {
            (conn.clone(), None)
        };
        let conn = &effective_conn;

        match conn.driver {
            DriverKind::Sqlite => Self::list_tables_sqlite(conn),
            DriverKind::Postgres => list_tables_postgres(conn).await,
            DriverKind::Mysql | DriverKind::MariaDb => list_tables_mysql(conn).await,
            DriverKind::DuckDb => crate::driver::DriverRegistry::list_tables_duckdb(conn).await,
            DriverKind::Redis => Err(CatableError::Driver {
                driver: "redis".into(),
                message: "Redis key-value store does not have relational tables.".into(),
            }),
        }
    }

    /// Lists tables and columns for the connection synchronously.
    pub fn list_tables(conn: &ConnectionConfig) -> Result<Vec<TableInfo>, CatableError> {
        let conn_clone = conn.clone();
        run_async(async move { Self::list_tables_async(&conn_clone).await })
    }

    /// Executes a SQL query after evaluating safety guardrails asynchronously.
    pub async fn execute_query_async(conn: &ConnectionConfig, sql: &str) -> QueryResult {
        let start = Instant::now();
        let verdict = GuardrailEngine::evaluate(sql, conn);

        if verdict.risk == QueryRisk::Blocked {
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

        let (effective_conn, _tunnel) = if conn.ssh_tunnel.as_ref().map_or(false, |t| t.enabled) {
            match crate::tunnel::open_tunnel(conn) {
                Ok(tun) => {
                    let mut c = conn.clone();
                    c.host = "127.0.0.1".into();
                    c.port = tun.local_port();
                    (c, Some(tun))
                }
                Err(e) => {
                    return QueryResult {
                        columns: vec![],
                        rows: vec![],
                        rows_affected: 0,
                        duration_ms: start.elapsed().as_millis() as u64,
                        error: Some(format!("SSH Tunnel failed: {}", e)),
                        guard_verdict: Some(verdict),
                    };
                }
            }
        } else {
            (conn.clone(), None)
        };
        let conn = &effective_conn;

        match conn.driver {
            DriverKind::Sqlite => Self::execute_query_sqlite(conn, sql, verdict, start),
            DriverKind::Postgres => execute_query_postgres(conn, sql, verdict, start).await,
            DriverKind::Mysql | DriverKind::MariaDb => {
                execute_query_mysql(conn, sql, verdict, start).await
            }
            DriverKind::DuckDb => {
                crate::driver::DriverRegistry::execute_query_duckdb(conn, sql, verdict, start).await
            }
            DriverKind::Redis => QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: 0,
                duration_ms: start.elapsed().as_millis() as u64,
                error: Some("Redis key-value driver does not support SQL execution.".into()),
                guard_verdict: Some(verdict),
            },
        }
    }

    /// Executes a SQL query after evaluating safety guardrails synchronously.
    pub fn execute_query(conn: &ConnectionConfig, sql: &str) -> QueryResult {
        let conn_clone = conn.clone();
        let sql_string = sql.to_string();
        run_async(async move { Self::execute_query_async(&conn_clone, &sql_string).await })
    }

    fn list_tables_sqlite(conn: &ConnectionConfig) -> Result<Vec<TableInfo>, CatableError> {
        let path = Self::resolve_sqlite_path(conn)?;
        let sqlite = Connection::open(&path).map_err(|e| CatableError::Vault(e.to_string()))?;

        let mut stmt = sqlite
            .prepare(
                "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name ASC",
            )
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        let table_rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| CatableError::Vault(e.to_string()))?;

        let mut tables = Vec::new();

        for item in table_rows {
            let (name, table_type) = item.map_err(|e| CatableError::Vault(e.to_string()))?;

            // Fetch columns via PRAGMA table_info
            let pragma_sql = format!("PRAGMA table_info(\"{}\")", name.replace('"', "\"\""));
            let mut col_stmt = sqlite
                .prepare(&pragma_sql)
                .map_err(|e| CatableError::Vault(e.to_string()))?;

            let cols = col_stmt
                .query_map([], |cr| {
                    Ok(TableColumnInfo {
                        name: cr.get(1)?,
                        data_type: cr.get(2)?,
                        not_null: cr.get::<_, i32>(3)? != 0,
                        is_pk: cr.get::<_, i32>(5)? != 0,
                    })
                })
                .map_err(|e| CatableError::Vault(e.to_string()))?
                .filter_map(|c| c.ok())
                .collect();

            // Fetch row count
            let count_sql = format!("SELECT count(*) FROM \"{}\"", name.replace('"', "\"\""));
            let row_count: u64 = sqlite
                .query_row(&count_sql, [], |r| r.get::<_, i64>(0))
                .map(|c| c.max(0) as u64)
                .unwrap_or(0);

            tables.push(TableInfo {
                name,
                table_type,
                columns: cols,
                row_count,
            });
        }

        Ok(tables)
    }

    fn execute_query_sqlite(
        conn: &ConnectionConfig,
        sql: &str,
        verdict: GuardVerdict,
        start: Instant,
    ) -> QueryResult {
        let path = match Self::resolve_sqlite_path(conn) {
            Ok(p) => p,
            Err(e) => {
                return QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                    duration_ms: start.elapsed().as_millis() as u64,
                    error: Some(format!("Failed to resolve database target: {}", e)),
                    guard_verdict: Some(verdict),
                }
            }
        };

        let sqlite = match Connection::open(&path) {
            Ok(c) => c,
            Err(e) => {
                return QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                    duration_ms: start.elapsed().as_millis() as u64,
                    error: Some(format!("Database connection failed: {}", e)),
                    guard_verdict: Some(verdict),
                }
            }
        };

        let trimmed = sql.trim();
        let upper = strip_leading_sql_comments(trimmed).to_uppercase();
        let is_select = upper.starts_with("SELECT")
            || upper.starts_with("PRAGMA")
            || upper.starts_with("EXPLAIN")
            || upper.starts_with("WITH");

        if is_select {
            let mut stmt = match sqlite.prepare(trimmed) {
                Ok(s) => s,
                Err(e) => {
                    return QueryResult {
                        columns: vec![],
                        rows: vec![],
                        rows_affected: 0,
                        duration_ms: start.elapsed().as_millis() as u64,
                        error: Some(format!("SQL Syntax / Prepare Error: {}", e)),
                        guard_verdict: Some(verdict),
                    }
                }
            };

            let col_names: Vec<String> = stmt
                .column_names()
                .into_iter()
                .map(|s| s.to_string())
                .collect();

            let col_count = col_names.len();
            let mut rows_out = Vec::new();

            let mut query_rows = match stmt.query([]) {
                Ok(r) => r,
                Err(e) => {
                    return QueryResult {
                        columns: col_names,
                        rows: vec![],
                        rows_affected: 0,
                        duration_ms: start.elapsed().as_millis() as u64,
                        error: Some(format!("Query Execution Error: {}", e)),
                        guard_verdict: Some(verdict),
                    }
                }
            };

            while let Ok(Some(row)) = query_rows.next() {
                let mut row_vals = Vec::with_capacity(col_count);
                for i in 0..col_count {
                    let val = match row.get_ref(i) {
                        Ok(ValueRef::Null) => serde_json::Value::Null,
                        Ok(ValueRef::Integer(n)) => serde_json::json!(n),
                        Ok(ValueRef::Real(f)) => serde_json::json!(f),
                        Ok(ValueRef::Text(t)) => {
                            serde_json::Value::String(String::from_utf8_lossy(t).to_string())
                        }
                        Ok(ValueRef::Blob(b)) => {
                            serde_json::Value::String(format!("<BLOB {} bytes>", b.len()))
                        }
                        Err(_) => serde_json::Value::Null,
                    };
                    row_vals.push(val);
                }
                rows_out.push(row_vals);

                // Bound result rows for desktop performance
                if rows_out.len() >= 10_000 {
                    break;
                }
            }

            let duration = start.elapsed().as_millis() as u64;
            let count = rows_out.len() as u64;

            QueryResult {
                columns: col_names,
                rows: rows_out,
                rows_affected: count,
                duration_ms: duration,
                error: None,
                guard_verdict: Some(verdict),
            }
        } else {
            // Mutation / DDL query
            match sqlite.execute(trimmed, params_from_iter(std::iter::empty::<&str>())) {
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
}



// ---------------------------------------------------------------------------
// PostgreSQL Implementation
// ---------------------------------------------------------------------------

async fn connect_postgres(conn: &ConnectionConfig) -> Result<tokio_postgres::Client, CatableError> {
    if conn.host.trim().is_empty() {
        return Err(CatableError::Driver {
            driver: "postgres".into(),
            message: "Host address cannot be empty.".into(),
        });
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

    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        pg_config.connect(tokio_postgres::NoTls),
    )
    .await
    {
        Ok(Ok((client, connection))) => {
            tokio::spawn(async move {
                let _ = connection.await;
            });
            if conn.read_only {
                let _ = client
                    .execute("SET default_transaction_read_only = on", &[])
                    .await;
            }
            Ok(client)
        }
        Ok(Err(err)) => {
            let err_str = err.to_string().to_lowercase();
            if err_str.contains("ssl") || err_str.contains("tls") {
                let connector = native_tls::TlsConnector::builder()
                    .danger_accept_invalid_certs(true)
                    .build()
                    .map_err(|te| CatableError::Driver {
                        driver: "postgres".into(),
                        message: format!("TLS initialization failed: {}", te),
                    })?;
                let tls = postgres_native_tls::MakeTlsConnector::new(connector);
                let (client, connection) = match tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    pg_config.connect(tls),
                )
                .await
                {
                    Ok(Ok(res)) => res,
                    Ok(Err(ce)) => {
                        return Err(CatableError::Driver {
                            driver: "postgres".into(),
                            message: ce.to_string(),
                        });
                    }
                    Err(_) => {
                        return Err(CatableError::Driver {
                            driver: "postgres".into(),
                            message: "Connection timed out".into(),
                        });
                    }
                };
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                if conn.read_only {
                    let _ = client
                        .execute("SET default_transaction_read_only = on", &[])
                        .await;
                }
                Ok(client)
            } else {
                Err(CatableError::Driver {
                    driver: "postgres".into(),
                    message: err.to_string(),
                })
            }
        }
        Err(_) => Err(CatableError::Driver {
            driver: "postgres".into(),
            message: "Connection timed out".into(),
        }),
    }
}

async fn list_tables_postgres(conn: &ConnectionConfig) -> Result<Vec<TableInfo>, CatableError> {
    let client = connect_postgres(conn).await?;

    let table_rows = client
        .query(
            "SELECT table_name, table_type \
             FROM information_schema.tables \
             WHERE table_schema NOT IN ('pg_catalog', 'information_schema') \
             ORDER BY table_name ASC",
            &[],
        )
        .await
        .map_err(|e| CatableError::Driver {
            driver: "postgres".into(),
            message: e.to_string(),
        })?;

    let col_rows = client
        .query(
            "SELECT \
                 c.table_name, \
                 c.column_name, \
                 c.data_type, \
                 c.is_nullable, \
                 CASE WHEN pk.column_name IS NOT NULL THEN true ELSE false END as is_pk \
             FROM information_schema.columns c \
             LEFT JOIN ( \
                 SELECT kcu.table_name, kcu.column_name \
                 FROM information_schema.table_constraints tc \
                 JOIN information_schema.key_column_usage kcu \
                     ON tc.constraint_name = kcu.constraint_name \
                     AND tc.table_schema = kcu.table_schema \
                 WHERE tc.constraint_type = 'PRIMARY KEY' \
                     AND tc.table_schema NOT IN ('pg_catalog', 'information_schema') \
             ) pk ON c.table_name = pk.table_name AND c.column_name = pk.column_name \
             WHERE c.table_schema NOT IN ('pg_catalog', 'information_schema') \
             ORDER BY c.table_name, c.ordinal_position",
            &[],
        )
        .await
        .map_err(|e| CatableError::Driver {
            driver: "postgres".into(),
            message: e.to_string(),
        })?;

    let stat_rows = client
        .query(
            "SELECT relname, COALESCE(n_live_tup, 0) as row_count \
             FROM pg_stat_user_tables",
            &[],
        )
        .await
        .unwrap_or_default();

    let mut row_counts: HashMap<String, u64> = HashMap::new();
    for r in stat_rows {
        let name: String = r.get(0);
        let count: i64 = r.get(1);
        row_counts.insert(name, count.max(0) as u64);
    }

    let mut col_map: HashMap<String, Vec<TableColumnInfo>> = HashMap::new();
    for cr in col_rows {
        let tbl: String = cr.get(0);
        let name: String = cr.get(1);
        let data_type: String = cr.get(2);
        let is_nullable: String = cr.get(3);
        let is_pk: bool = cr.get(4);

        col_map.entry(tbl).or_default().push(TableColumnInfo {
            name,
            data_type,
            is_pk,
            not_null: is_nullable.to_uppercase() == "NO",
        });
    }

    let mut tables = Vec::new();
    for tr in table_rows {
        let name: String = tr.get(0);
        let table_type: String = tr.get(1);
        let columns = col_map.remove(&name).unwrap_or_default();
        let row_count = row_counts.get(&name).copied().unwrap_or(0);

        tables.push(TableInfo {
            name,
            table_type,
            columns,
            row_count,
        });
    }

    Ok(tables)
}

async fn execute_query_postgres(
    conn: &ConnectionConfig,
    sql: &str,
    verdict: GuardVerdict,
    start: Instant,
) -> QueryResult {
    let client = match connect_postgres(conn).await {
        Ok(c) => c,
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
    let upper = strip_leading_sql_comments(trimmed).to_uppercase();
    let is_select = upper.starts_with("SELECT")
        || upper.starts_with("EXPLAIN")
        || upper.starts_with("SHOW")
        || upper.starts_with("WITH");

    if is_select {
        match client.query(trimmed, &[]).await {
            Ok(rows) => {
                let col_names: Vec<String> = if let Some(first_row) = rows.first() {
                    first_row
                        .columns()
                        .iter()
                        .map(|c| c.name().to_string())
                        .collect()
                } else {
                    match client.prepare(trimmed).await {
                        Ok(stmt) => stmt
                            .columns()
                            .iter()
                            .map(|c| c.name().to_string())
                            .collect(),
                        Err(_) => vec![],
                    }
                };

                let col_count = col_names.len();
                let mut rows_out = Vec::new();

                for row in rows.iter().take(10_000) {
                    let mut row_vals = Vec::with_capacity(col_count);
                    for i in 0..col_count {
                        let val = pg_value_to_json(row, i);
                        row_vals.push(val);
                    }
                    rows_out.push(row_vals);
                }

                let duration = start.elapsed().as_millis() as u64;
                let count = rows_out.len() as u64;

                QueryResult {
                    columns: col_names,
                    rows: rows_out,
                    rows_affected: count,
                    duration_ms: duration,
                    error: None,
                    guard_verdict: Some(verdict),
                }
            }
            Err(e) => QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: 0,
                duration_ms: start.elapsed().as_millis() as u64,
                error: Some(format!("Query execution failed: {}", e)),
                guard_verdict: Some(verdict),
            },
        }
    } else {
        match client.execute(trimmed, &[]).await {
            Ok(affected) => QueryResult {
                columns: vec!["rows_affected".into()],
                rows: vec![vec![serde_json::json!(affected)]],
                rows_affected: affected,
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

fn pg_value_to_json(row: &tokio_postgres::Row, idx: usize) -> serde_json::Value {
    use tokio_postgres::types::Type;
    let col = &row.columns()[idx];
    let col_type = col.type_();

    match *col_type {
        Type::BOOL => row
            .try_get::<_, Option<bool>>(idx)
            .ok()
            .flatten()
            .map(serde_json::Value::Bool)
            .unwrap_or(serde_json::Value::Null),
        Type::INT2 => row
            .try_get::<_, Option<i16>>(idx)
            .ok()
            .flatten()
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null),
        Type::INT4 => row
            .try_get::<_, Option<i32>>(idx)
            .ok()
            .flatten()
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null),
        Type::INT8 => row
            .try_get::<_, Option<i64>>(idx)
            .ok()
            .flatten()
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null),
        Type::FLOAT4 => row
            .try_get::<_, Option<f32>>(idx)
            .ok()
            .flatten()
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null),
        Type::FLOAT8 => row
            .try_get::<_, Option<f64>>(idx)
            .ok()
            .flatten()
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => row
            .try_get::<_, Option<String>>(idx)
            .ok()
            .flatten()
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null),
        Type::JSON | Type::JSONB => row
            .try_get::<_, Option<serde_json::Value>>(idx)
            .ok()
            .flatten()
            .unwrap_or(serde_json::Value::Null),
        Type::DATE => row
            .try_get::<_, Option<chrono::NaiveDate>>(idx)
            .ok()
            .flatten()
            .map(|d| serde_json::json!(d.to_string()))
            .unwrap_or(serde_json::Value::Null),
        Type::TIME => row
            .try_get::<_, Option<chrono::NaiveTime>>(idx)
            .ok()
            .flatten()
            .map(|t| serde_json::json!(t.to_string()))
            .unwrap_or(serde_json::Value::Null),
        Type::TIMESTAMP => row
            .try_get::<_, Option<chrono::NaiveDateTime>>(idx)
            .ok()
            .flatten()
            .map(|dt| serde_json::json!(dt.to_string()))
            .unwrap_or(serde_json::Value::Null),
        Type::TIMESTAMPTZ => row
            .try_get::<_, Option<chrono::DateTime<chrono::Utc>>>(idx)
            .ok()
            .flatten()
            .map(|dt| serde_json::json!(dt.to_rfc3339()))
            .or_else(|| {
                row.try_get::<_, Option<chrono::NaiveDateTime>>(idx)
                    .ok()
                    .flatten()
                    .map(|dt| serde_json::json!(dt.to_string()))
            })
            .unwrap_or(serde_json::Value::Null),
        Type::BYTEA => row
            .try_get::<_, Option<Vec<u8>>>(idx)
            .ok()
            .flatten()
            .map(|b| serde_json::Value::String(format!("<BLOB {} bytes>", b.len())))
            .unwrap_or(serde_json::Value::Null),
        _ => {
            if let Ok(Some(s)) = row.try_get::<_, Option<String>>(idx) {
                serde_json::Value::String(s)
            } else if let Ok(Some(n)) = row.try_get::<_, Option<i64>>(idx) {
                serde_json::json!(n)
            } else if let Ok(Some(f)) = row.try_get::<_, Option<f64>>(idx) {
                serde_json::json!(f)
            } else {
                serde_json::Value::Null
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MySQL / MariaDB Implementation
// ---------------------------------------------------------------------------

async fn connect_mysql(conn: &ConnectionConfig) -> Result<mysql_async::Conn, CatableError> {
    use mysql_async::prelude::Queryable;

    if conn.host.trim().is_empty() {
        return Err(CatableError::Driver {
            driver: if conn.driver == DriverKind::MariaDb {
                "mariadb".into()
            } else {
                "mysql".into()
            },
            message: "Host address cannot be empty.".into(),
        });
    }

    let mut opts_builder = mysql_async::OptsBuilder::default();
    opts_builder = opts_builder.ip_or_hostname(conn.host.clone());
    opts_builder = opts_builder.tcp_port(conn.effective_port());

    if !conn.user.is_empty() {
        opts_builder = opts_builder.user(Some(conn.user.clone()));
    }
    if let Some(ref pw) = conn.password {
        opts_builder = opts_builder.pass(Some(pw.clone()));
    }
    if !conn.database.is_empty() {
        opts_builder = opts_builder.db_name(Some(conn.database.clone()));
    }

    let opts: mysql_async::Opts = opts_builder.into();
    let mut mysql_conn = match tokio::time::timeout(std::time::Duration::from_secs(5), mysql_async::Conn::new(opts)).await {
        Ok(Ok(conn)) => conn,
        Ok(Err(e)) => {
            return Err(CatableError::Driver {
                driver: if conn.driver == DriverKind::MariaDb {
                    "mariadb".into()
                } else {
                    "mysql".into()
                },
                message: e.to_string(),
            });
        }
        Err(_) => {
            return Err(CatableError::Driver {
                driver: if conn.driver == DriverKind::MariaDb {
                    "mariadb".into()
                } else {
                    "mysql".into()
                },
                message: "Connection timed out".into(),
            });
        }
    };

    if conn.read_only {
        let _ = mysql_conn.query_drop("SET SESSION TRANSACTION READ ONLY").await;
    }

    Ok(mysql_conn)
}

async fn list_tables_mysql(conn: &ConnectionConfig) -> Result<Vec<TableInfo>, CatableError> {
    use mysql_async::prelude::Queryable;
    let mut mysql_conn = connect_mysql(conn).await?;

    let table_rows: Vec<(String, String, Option<u64>)> = mysql_conn
        .query(
            "SELECT TABLE_NAME, TABLE_TYPE, TABLE_ROWS \
             FROM information_schema.tables \
             WHERE TABLE_SCHEMA = DATABASE() \
             ORDER BY TABLE_NAME ASC",
        )
        .await
        .map_err(|e| CatableError::Driver {
            driver: "mysql".into(),
            message: e.to_string(),
        })?;

    let col_rows: Vec<(String, String, String, String, String)> = mysql_conn
        .query(
            "SELECT TABLE_NAME, COLUMN_NAME, DATA_TYPE, IS_NULLABLE, COLUMN_KEY \
             FROM information_schema.columns \
             WHERE TABLE_SCHEMA = DATABASE() \
             ORDER BY TABLE_NAME, ORDINAL_POSITION",
        )
        .await
        .map_err(|e| CatableError::Driver {
            driver: "mysql".into(),
            message: e.to_string(),
        })?;

    let mut col_map: HashMap<String, Vec<TableColumnInfo>> = HashMap::new();
    for (tbl, col_name, data_type, is_nullable, col_key) in col_rows {
        col_map.entry(tbl).or_default().push(TableColumnInfo {
            name: col_name,
            data_type,
            is_pk: col_key == "PRI",
            not_null: is_nullable.to_uppercase() == "NO",
        });
    }

    let mut tables = Vec::new();
    for (name, table_type, row_count_opt) in table_rows {
        let columns = col_map.remove(&name).unwrap_or_default();
        let row_count = row_count_opt.unwrap_or(0);
        tables.push(TableInfo {
            name,
            table_type,
            columns,
            row_count,
        });
    }

    Ok(tables)
}

async fn execute_query_mysql(
    conn: &ConnectionConfig,
    sql: &str,
    verdict: GuardVerdict,
    start: Instant,
) -> QueryResult {
    use mysql_async::prelude::Queryable;
    let mut mysql_conn = match connect_mysql(conn).await {
        Ok(c) => c,
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
    let upper = strip_leading_sql_comments(trimmed).to_uppercase();
    let is_select = upper.starts_with("SELECT")
        || upper.starts_with("SHOW")
        || upper.starts_with("DESCRIBE")
        || upper.starts_with("DESC ")
        || upper.starts_with("EXPLAIN")
        || upper.starts_with("WITH");

    if is_select {
        match mysql_conn.query_iter(trimmed).await {
            Ok(mut query_res) => {
                let col_names: Vec<String> = query_res
                    .columns()
                    .as_ref()
                    .map(|cols| cols.iter().map(|c| c.name_str().to_string()).collect())
                    .unwrap_or_default();

                match query_res.collect::<mysql_async::Row>().await {
                    Ok(raw_rows) => {
                        let mut rows_out = Vec::new();
                        for row in raw_rows.into_iter().take(10_000) {
                            let values: Vec<mysql_async::Value> = row.unwrap();
                            let json_row = values.into_iter().map(mysql_value_to_json).collect();
                            rows_out.push(json_row);
                        }

                        let duration = start.elapsed().as_millis() as u64;
                        let count = rows_out.len() as u64;

                        QueryResult {
                            columns: col_names,
                            rows: rows_out,
                            rows_affected: count,
                            duration_ms: duration,
                            error: None,
                            guard_verdict: Some(verdict),
                        }
                    }
                    Err(e) => QueryResult {
                        columns: col_names,
                        rows: vec![],
                        rows_affected: 0,
                        duration_ms: start.elapsed().as_millis() as u64,
                        error: Some(format!("Failed to read result rows: {}", e)),
                        guard_verdict: Some(verdict),
                    },
                }
            }
            Err(e) => QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: 0,
                duration_ms: start.elapsed().as_millis() as u64,
                error: Some(format!("Query execution failed: {}", e)),
                guard_verdict: Some(verdict),
            },
        }
    } else {
        match mysql_conn.query_drop(trimmed).await {
            Ok(_) => {
                let affected = mysql_conn.affected_rows();
                QueryResult {
                    columns: vec!["rows_affected".into()],
                    rows: vec![vec![serde_json::json!(affected)]],
                    rows_affected: affected,
                    duration_ms: start.elapsed().as_millis() as u64,
                    error: None,
                    guard_verdict: Some(verdict),
                }
            }
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

fn mysql_value_to_json(val: mysql_async::Value) -> serde_json::Value {
    match val {
        mysql_async::Value::NULL => serde_json::Value::Null,
        mysql_async::Value::Bytes(b) => match String::from_utf8(b) {
            Ok(s) => serde_json::Value::String(s),
            Err(e) => {
                let bytes = e.into_bytes();
                serde_json::Value::String(format!("<BLOB {} bytes>", bytes.len()))
            }
        },
        mysql_async::Value::Int(n) => serde_json::json!(n),
        mysql_async::Value::UInt(u) => serde_json::json!(u),
        mysql_async::Value::Float(f) => serde_json::json!(f),
        mysql_async::Value::Double(d) => serde_json::json!(d),
        mysql_async::Value::Date(y, m, d, h, min, s, u) => {
            if h == 0 && min == 0 && s == 0 && u == 0 {
                serde_json::Value::String(format!("{:04}-{:02}-{:02}", y, m, d))
            } else if u > 0 {
                serde_json::Value::String(format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}",
                    y, m, d, h, min, s, u
                ))
            } else {
                serde_json::Value::String(format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                    y, m, d, h, min, s
                ))
            }
        }
        mysql_async::Value::Time(neg, d, h, m, s, _u) => {
            let sign = if neg { "-" } else { "" };
            if d > 0 {
                serde_json::Value::String(format!("{}{}:{:02}:{:02}:{:02}", sign, d, h, m, s))
            } else {
                serde_json::Value::String(format!("{}{:02}:{:02}:{:02}", sign, h, m, s))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn strip_leading_sql_comments(sql: &str) -> &str {
    let mut s = sql.trim();
    loop {
        if s.starts_with("--") {
            if let Some(pos) = s.find('\n') {
                s = s[pos + 1..].trim();
            } else {
                return "";
            }
        } else if s.starts_with("/*") {
            if let Some(pos) = s.find("*/") {
                s = s[pos + 2..].trim();
            } else {
                return "";
            }
        } else {
            break;
        }
    }
    s
}

pub(crate) fn run_async<F, R>(future: F) -> R
where
    F: std::future::Future<Output = R> + Send + 'static,
    R: Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread {
            tokio::task::block_in_place(|| handle.block_on(future))
        } else {
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build temporary tokio runtime");
                rt.block_on(future)
            })
            .join()
            .expect("background executor thread panicked")
        }
    } else {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build temporary tokio runtime");
        rt.block_on(future)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::Environment;

    fn test_conn() -> ConnectionConfig {
        ConnectionConfig {
            id: "sandbox".into(),
            name: "Sandbox DB".into(),
            driver: DriverKind::Sqlite,
            environment: Environment::Development,
            host: "".into(),
            port: 0,
            database: ":memory:".into(),
            user: "".into(),
            password: None,
            read_only: false,
            ssh_tunnel: None,
        }
    }

    #[test]
    fn test_sqlite_query_execution() {
        let conn = test_conn();
        let res = QueryExecutor::execute_query(&conn, "SELECT 1 + 1 AS sum, 'hello' AS greeting");
        assert!(res.error.is_none());
        assert_eq!(res.columns, vec!["sum", "greeting"]);
        assert_eq!(res.rows.len(), 1);
        assert_eq!(res.rows[0][0], serde_json::json!(2));
        assert_eq!(res.rows[0][1], serde_json::json!("hello"));
    }

    #[test]
    fn test_list_tables() {
        let conn = test_conn();
        let tables = QueryExecutor::list_tables(&conn).unwrap();
        assert!(!tables.is_empty());
        assert!(tables.iter().any(|t| t.name == "users"));
    }

    #[test]
    fn test_connection_testing() {
        let conn = test_conn();
        let res = QueryExecutor::test_connection(&conn);
        assert!(res.success);
        assert!(res.server_version.contains("SQLite"));
    }

    #[tokio::test]
    async fn test_postgres_unreachable_query() {
        let conn = ConnectionConfig {
            id: "pg-offline".into(),
            name: "PostgreSQL Offline".into(),
            driver: DriverKind::Postgres,
            environment: Environment::Development,
            host: "127.0.0.1".into(),
            port: 59998,
            database: "test".into(),
            user: "postgres".into(),
            password: None,
            read_only: false,
            ssh_tunnel: None,
        };
        let res = QueryExecutor::execute_query_async(&conn, "SELECT 1").await;
        assert!(res.error.is_some());
        assert!(res.error.unwrap().contains("Database connection failed"));
    }

    #[tokio::test]
    async fn test_mysql_unreachable_query() {
        let conn = ConnectionConfig {
            id: "mysql-offline".into(),
            name: "MySQL Offline".into(),
            driver: DriverKind::Mysql,
            environment: Environment::Development,
            host: "127.0.0.1".into(),
            port: 59998,
            database: "test".into(),
            user: "root".into(),
            password: None,
            read_only: false,
            ssh_tunnel: None,
        };
        let res = QueryExecutor::execute_query_async(&conn, "SELECT 1").await;
        assert!(res.error.is_some());
        assert!(res.error.unwrap().contains("Database connection failed"));
    }

    #[test]
    fn test_guardrail_blocks_dangerous_queries_before_remote_connect() {
        let conn = ConnectionConfig {
            id: "pg-prod".into(),
            name: "PostgreSQL Prod".into(),
            driver: DriverKind::Postgres,
            environment: Environment::Production,
            host: "127.0.0.1".into(),
            port: 5432,
            database: "prod_db".into(),
            user: "admin".into(),
            password: None,
            read_only: false,
            ssh_tunnel: None,
        };
        // Unbounded DELETE on prod must be blocked instantly without network call
        let res = QueryExecutor::execute_query(&conn, "DELETE FROM customers");
        assert!(res.error.is_some());
        assert!(res.error.unwrap().contains("Query Blocked by Guardrail"));
        assert_eq!(res.guard_verdict.unwrap().risk, QueryRisk::Blocked);

        // DROP TABLE on prod must be blocked instantly
        let res2 = QueryExecutor::execute_query(&conn, "DROP TABLE sensitive_data");
        assert!(res2.error.is_some());
        assert!(res2.error.unwrap().contains("Query Blocked by Guardrail"));

        // Read-only connection must block writes
        let mut read_only_conn = conn.clone();
        read_only_conn.read_only = true;
        let res3 = QueryExecutor::execute_query(
            &read_only_conn,
            "INSERT INTO audit_log (event) VALUES ('test')",
        );
        assert!(res3.error.is_some());
        assert!(res3.error.unwrap().contains("Query Blocked by Guardrail"));
    }
}
