//! Long-lived database sessions.
//!
//! Before sessions, every query opened a fresh connection (and a fresh SSH
//! tunnel), ran, and closed it. A [`Session`] instead keeps, per opened
//! connection:
//!
//! * one SSH tunnel (when the connection uses one) for the session's lifetime,
//! * a small pool of already-authenticated PostgreSQL / MySQL clients.
//!
//! Secrets never leave Rust: the frontend opens a session by `connection_id`,
//! the decrypted [`ConnectionConfig`] lives only inside the [`Session`], and
//! every later call carries only the opaque session id.
//!
//! SQLite and DuckDB are local files, so they are not pooled; they run through
//! the existing per-call executor on the blocking pool.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore};

use crate::connection::{ConnectionConfig, DriverKind};
use crate::error::CatableError;
use crate::executor::{
    QueryExecutor, QueryResult, TableInfo, connect_mysql, connect_postgres, describe_mysql,
    describe_postgres, run_query_mysql, run_query_postgres, strip_leading_sql_comments,
};
use crate::guard::{GuardVerdict, GuardrailEngine, QueryRisk};
use crate::mutation::{self, ApplyResult, PlannedStatement, RowChangeSet};
use crate::schema::{self, TableStructure};
use crate::sql_builder::{BrowseRequest, build_count, build_select};
use crate::tunnel::TunnelSession;

/// Maximum simultaneous connections per session.
pub const DEFAULT_POOL_SIZE: usize = 4;

// ---------------------------------------------------------------------------
// Generic pool
// ---------------------------------------------------------------------------

/// Idle clients plus a semaphore that bounds how many may be leased at once.
struct Pool<C> {
    idle: Mutex<Vec<C>>,
    permits: Arc<Semaphore>,
}

impl<C> Pool<C> {
    fn new(size: usize) -> Arc<Self> {
        Arc::new(Self {
            idle: Mutex::new(Vec::new()),
            permits: Arc::new(Semaphore::new(size.max(1))),
        })
    }

    /// Waits for a free slot, then hands out an idle client that passes
    /// `usable`, or builds a new one with `make`.
    async fn lease<F, Fut>(
        self: &Arc<Self>,
        usable: impl Fn(&C) -> bool,
        make: F,
    ) -> Result<Lease<C>, CatableError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<C, CatableError>>,
    {
        let permit =
            self.permits
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| CatableError::Driver {
                    driver: "session".into(),
                    message: "session pool is closed".into(),
                })?;

        let reused = loop {
            let candidate = self.idle.lock().pop();
            match candidate {
                Some(client) if usable(&client) => break Some(client),
                Some(_stale) => continue,
                None => break None,
            }
        };

        let client = match reused {
            Some(client) => client,
            None => make().await?,
        };

        Ok(Lease {
            client: Some(client),
            pool: Arc::clone(self),
            _permit: permit,
            discard: false,
        })
    }

    /// Drops every idle client (used after a connection-lost error: the rest
    /// of the idle set is almost certainly dead too).
    fn clear_idle(&self) {
        self.idle.lock().clear();
    }

    #[cfg(test)]
    fn idle_len(&self) -> usize {
        self.idle.lock().len()
    }
}

/// A client on loan from a [`Pool`]; returned to the idle set on drop unless
/// marked for discard.
struct Lease<C> {
    client: Option<C>,
    pool: Arc<Pool<C>>,
    _permit: OwnedSemaphorePermit,
    discard: bool,
}

impl<C> Lease<C> {
    fn client(&mut self) -> &mut C {
        self.client
            .as_mut()
            .expect("a lease holds its client until it is dropped")
    }

    fn discard(&mut self) {
        self.discard = true;
    }
}

impl<C> Drop for Lease<C> {
    fn drop(&mut self) {
        if let Some(client) = self.client.take()
            && !self.discard
        {
            self.pool.idle.lock().push(client);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// True when an error message means the underlying connection is gone (server
/// restart, network drop, idle timeout) rather than a problem with the query.
fn connection_lost_msg(message: &str) -> bool {
    let m = message.to_lowercase();
    [
        "gone away",
        "broken pipe",
        "connection reset",
        "connection closed",
        "connection lost",
        "is closed",
        "server closed",
        "terminating connection",
        "not connected",
        "unexpected eof",
        "end of file",
        "lost connection",
        "was killed",
        "connection aborted",
    ]
    .iter()
    .any(|needle| m.contains(needle))
}

fn connection_lost(res: &QueryResult) -> bool {
    res.error.as_deref().is_some_and(connection_lost_msg)
}

/// Statements that are safe to retry once on a stale connection.
fn is_read_statement(sql: &str) -> bool {
    let upper = strip_leading_sql_comments(sql.trim()).to_uppercase();
    [
        "SELECT", "SHOW", "DESCRIBE", "DESC ", "EXPLAIN", "PRAGMA", "WITH",
    ]
    .iter()
    .any(|p| upper.starts_with(p))
}

fn error_result(message: String, verdict: Option<GuardVerdict>, start: Instant) -> QueryResult {
    QueryResult {
        truncated: false,
        columns: vec![],
        rows: vec![],
        rows_affected: 0,
        duration_ms: start.elapsed().as_millis() as u64,
        error: Some(message),
        guard_verdict: verdict,
    }
}

fn uses_network(driver: DriverKind) -> bool {
    matches!(
        driver,
        DriverKind::Postgres | DriverKind::Mysql | DriverKind::MariaDb | DriverKind::Redis
    )
}

fn edit_error(
    driver: &str,
    index: usize,
    statement: &PlannedStatement,
    e: impl std::fmt::Display,
) -> CatableError {
    CatableError::Driver {
        driver: driver.into(),
        message: format!(
            "Change {} failed: {e}\n{}\nNothing was saved.",
            index + 1,
            statement.sql
        ),
    }
}

async fn apply_postgres(
    client: &tokio_postgres::Client,
    plan: &[PlannedStatement],
) -> Result<u64, CatableError> {
    let fail = |e: tokio_postgres::Error| CatableError::Driver {
        driver: "postgres".into(),
        message: e.to_string(),
    };
    client.batch_execute("BEGIN").await.map_err(fail)?;
    let mut total = 0u64;
    for (index, statement) in plan.iter().enumerate() {
        match client.execute(statement.sql.as_str(), &[]).await {
            Ok(n) if statement.expect_one_row && n != 1 => {
                let _ = client.batch_execute("ROLLBACK").await;
                return Err(mutation::unexpected_row_count(index, statement, n));
            }
            Ok(n) => total += n,
            Err(e) => {
                let _ = client.batch_execute("ROLLBACK").await;
                return Err(edit_error("postgres", index, statement, e));
            }
        }
    }
    client
        .batch_execute("COMMIT")
        .await
        .map_err(|e| CatableError::Driver {
            driver: "postgres".into(),
            message: format!("Commit failed: {e}. Nothing was saved."),
        })?;
    Ok(total)
}

async fn apply_mysql(
    conn: &mut mysql_async::Conn,
    plan: &[PlannedStatement],
) -> Result<u64, CatableError> {
    use mysql_async::prelude::Queryable;
    let fail = |e: mysql_async::Error| CatableError::Driver {
        driver: "mysql".into(),
        message: e.to_string(),
    };
    conn.query_drop("START TRANSACTION").await.map_err(fail)?;
    let mut total = 0u64;
    for (index, statement) in plan.iter().enumerate() {
        match conn.query_drop(statement.sql.as_str()).await {
            Ok(()) => {
                let n = conn.affected_rows();
                if statement.expect_one_row && n != 1 {
                    let _ = conn.query_drop("ROLLBACK").await;
                    return Err(mutation::unexpected_row_count(index, statement, n));
                }
                total += n;
            }
            Err(e) => {
                let _ = conn.query_drop("ROLLBACK").await;
                return Err(edit_error("mysql", index, statement, e));
            }
        }
    }
    conn.query_drop("COMMIT")
        .await
        .map_err(|e| CatableError::Driver {
            driver: "mysql".into(),
            message: format!("Commit failed: {e}. Nothing was saved."),
        })?;
    Ok(total)
}

fn apply_sqlite(cfg: &ConnectionConfig, plan: &[PlannedStatement]) -> Result<u64, CatableError> {
    let fail = |e: rusqlite::Error| CatableError::Driver {
        driver: "sqlite".into(),
        message: e.to_string(),
    };
    let path = QueryExecutor::resolve_sqlite_path(cfg)?;
    let mut db = rusqlite::Connection::open(path).map_err(fail)?;
    let tx = db.transaction().map_err(fail)?;
    let mut total = 0u64;
    for (index, statement) in plan.iter().enumerate() {
        // Dropping `tx` on an early return rolls the whole set back.
        let n = tx
            .execute(statement.sql.as_str(), [])
            .map_err(|e| edit_error("sqlite", index, statement, e))? as u64;
        if statement.expect_one_row && n != 1 {
            return Err(mutation::unexpected_row_count(index, statement, n));
        }
        total += n;
    }
    tx.commit().map_err(fail)?;
    Ok(total)
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// What the UI is allowed to know about a session (no secrets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub connection_id: String,
    pub name: String,
    pub driver: DriverKind,
    pub tunneled: bool,
    /// How long opening took (tunnel + first handshake), in milliseconds.
    pub open_ms: u64,
}

/// One page of a table, plus the (optional) exact total for the same filters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseResult {
    pub result: QueryResult,
    /// `COUNT(*)` with the request's filters; `None` when not requested or it failed.
    pub total: Option<u64>,
    /// The SELECT that produced `result` (shown to the user as "View SQL").
    pub sql: String,
}

fn json_to_u64(v: &serde_json::Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_i64().map(|n| n.max(0) as u64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

pub struct Session {
    id: String,
    connection_id: String,
    name: String,
    driver: DriverKind,
    /// Effective config: host/port already point at the local tunnel end when
    /// a tunnel is in use, and `ssh_tunnel` is cleared so nothing re-tunnels.
    config: ConnectionConfig,
    tunneled: bool,
    open_ms: u64,
    /// Held only to keep the tunnel alive; closed when the session drops.
    _tunnel: Option<TunnelSession>,
    pg: Arc<Pool<tokio_postgres::Client>>,
    mysql: Arc<Pool<mysql_async::Conn>>,
}

impl Session {
    /// Opens the tunnel (if any) and performs a first real handshake so the UI
    /// gets a precise error at connect time instead of at the first query.
    pub async fn open(cfg: ConnectionConfig) -> Result<Arc<Session>, CatableError> {
        let started = Instant::now();

        let mut effective = cfg.clone();
        let mut tunnel = None;
        if uses_network(cfg.driver) && cfg.ssh_tunnel.as_ref().is_some_and(|t| t.enabled) {
            let t = crate::tunnel::open_tunnel_async(&cfg).await?;
            effective.host = "127.0.0.1".into();
            effective.port = t.local_port();
            tunnel = Some(t);
        }
        effective.ssh_tunnel = None;

        let mut session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            connection_id: cfg.id.clone(),
            name: cfg.name.clone(),
            driver: cfg.driver,
            config: effective,
            tunneled: tunnel.is_some(),
            open_ms: 0,
            _tunnel: tunnel,
            pg: Pool::new(DEFAULT_POOL_SIZE),
            mysql: Pool::new(DEFAULT_POOL_SIZE),
        };

        session.probe().await?;
        session.open_ms = started.elapsed().as_millis() as u64;
        Ok(Arc::new(session))
    }

    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            session_id: self.id.clone(),
            connection_id: self.connection_id.clone(),
            name: self.name.clone(),
            driver: self.driver,
            tunneled: self.tunneled,
            open_ms: self.open_ms,
        }
    }

    /// Connects once. For pooled drivers the client goes straight back to the
    /// idle set, so the first query finds a warm connection.
    async fn probe(&self) -> Result<(), CatableError> {
        match self.driver {
            DriverKind::Postgres => {
                let _warm = self.lease_pg().await?;
                Ok(())
            }
            DriverKind::Mysql | DriverKind::MariaDb => {
                let _warm = self.lease_mysql().await?;
                Ok(())
            }
            _ => {
                let res = crate::driver::test_connection(&self.config).await;
                if res.success {
                    Ok(())
                } else {
                    Err(CatableError::Driver {
                        driver: format!("{:?}", self.driver).to_lowercase(),
                        message: res.message,
                    })
                }
            }
        }
    }

    async fn lease_pg(&self) -> Result<Lease<tokio_postgres::Client>, CatableError> {
        let cfg = &self.config;
        self.pg
            .lease(
                |c: &tokio_postgres::Client| !c.is_closed(),
                || connect_postgres(cfg),
            )
            .await
    }

    async fn lease_mysql(&self) -> Result<Lease<mysql_async::Conn>, CatableError> {
        let cfg = &self.config;
        self.mysql
            .lease(|_c: &mysql_async::Conn| true, || connect_mysql(cfg))
            .await
    }

    fn clear_idle(&self) {
        self.pg.clear_idle();
        self.mysql.clear_idle();
    }

    /// Guardrail check, then run on a pooled connection. A read-only statement
    /// that hits a dead connection is retried once on a fresh one.
    pub async fn execute(&self, sql: &str) -> QueryResult {
        let start = Instant::now();
        let verdict = GuardrailEngine::evaluate(sql, &self.config);
        if verdict.risk == QueryRisk::Blocked {
            let reason = verdict.reason.clone().unwrap_or_default();
            return error_result(
                format!("Query Blocked by Guardrail: {reason}"),
                Some(verdict),
                start,
            );
        }

        match self.driver {
            DriverKind::Postgres | DriverKind::Mysql | DriverKind::MariaDb => {
                let mut res = self.run_pooled(sql, verdict.clone(), start).await;
                if is_read_statement(sql) && connection_lost(&res) {
                    self.clear_idle();
                    res = self.run_pooled(sql, verdict, start).await;
                }
                res
            }
            _ => QueryExecutor::execute_query_async(&self.config, sql).await,
        }
    }

    async fn run_pooled(&self, sql: &str, verdict: GuardVerdict, start: Instant) -> QueryResult {
        match self.driver {
            DriverKind::Postgres => match self.lease_pg().await {
                Ok(mut lease) => {
                    let res = run_query_postgres(lease.client(), sql, verdict, start).await;
                    if connection_lost(&res) {
                        lease.discard();
                    }
                    res
                }
                Err(e) => error_result(
                    format!("Database connection failed: {e}"),
                    Some(verdict),
                    start,
                ),
            },
            DriverKind::Mysql | DriverKind::MariaDb => match self.lease_mysql().await {
                Ok(mut lease) => {
                    let res = run_query_mysql(lease.client(), sql, verdict, start).await;
                    if connection_lost(&res) {
                        lease.discard();
                    }
                    res
                }
                Err(e) => error_result(
                    format!("Database connection failed: {e}"),
                    Some(verdict),
                    start,
                ),
            },
            _ => error_result(
                "This driver does not use a connection pool.".into(),
                Some(verdict),
                start,
            ),
        }
    }

    /// Tables + columns, via a pooled connection (retried once if it was stale).
    pub async fn list_tables(&self) -> Result<Vec<TableInfo>, CatableError> {
        let first = self.list_tables_once().await;
        let lost = matches!(&first, Err(e) if connection_lost_msg(&e.to_string()));
        if lost {
            self.clear_idle();
            self.list_tables_once().await
        } else {
            first
        }
    }

    async fn list_tables_once(&self) -> Result<Vec<TableInfo>, CatableError> {
        match self.driver {
            DriverKind::Postgres => {
                let mut lease = self.lease_pg().await?;
                let res = describe_postgres(lease.client()).await;
                if matches!(&res, Err(e) if connection_lost_msg(&e.to_string())) {
                    lease.discard();
                }
                res
            }
            DriverKind::Mysql | DriverKind::MariaDb => {
                let mut lease = self.lease_mysql().await?;
                let res = describe_mysql(lease.client()).await;
                if matches!(&res, Err(e) if connection_lost_msg(&e.to_string())) {
                    lease.discard();
                }
                res
            }
            _ => QueryExecutor::list_tables_async(&self.config).await,
        }
    }

    /// The SELECT for `req` in this session's dialect, without running it.
    pub fn build_select_sql(&self, req: &BrowseRequest) -> Result<String, CatableError> {
        build_select(self.driver, req)
    }

    /// Runs one page of a table (server-side filter / sort / paging) and, if
    /// asked, the exact filtered total. All SQL is built here, never in the UI.
    pub async fn browse(&self, req: &BrowseRequest) -> Result<BrowseResult, CatableError> {
        let sql = build_select(self.driver, req)?;
        let result = self.execute(&sql).await;

        let mut total = None;
        if req.with_total && result.error.is_none() {
            let count_sql = build_count(self.driver, req)?;
            let counted = self.execute(&count_sql).await;
            if counted.error.is_none() {
                total = counted
                    .rows
                    .first()
                    .and_then(|row| row.first())
                    .and_then(json_to_u64);
            }
        }

        Ok(BrowseResult { result, total, sql })
    }

    /// Columns, keys, foreign keys, indexes and DDL of one table, from the engine's own catalog.
    pub async fn describe_structure(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<TableStructure, CatableError> {
        match self.driver {
            DriverKind::Postgres => {
                let mut lease = self.lease_pg().await?;
                let out = schema::structure_postgres(lease.client(), table, schema).await;
                if matches!(&out, Err(e) if connection_lost_msg(&e.to_string())) {
                    lease.discard();
                }
                out
            }
            DriverKind::Mysql | DriverKind::MariaDb => {
                let mut lease = self.lease_mysql().await?;
                let out = schema::structure_mysql(lease.client(), self.driver, table, schema).await;
                if matches!(&out, Err(e) if connection_lost_msg(&e.to_string())) {
                    lease.discard();
                }
                out
            }
            DriverKind::Sqlite => {
                let cfg = self.config.clone();
                let (table, schema) = (table.to_string(), schema.map(str::to_string));
                tokio::task::spawn_blocking(move || {
                    let path = QueryExecutor::resolve_sqlite_path(&cfg)?;
                    let db =
                        rusqlite::Connection::open(path).map_err(|e| CatableError::Driver {
                            driver: "sqlite".into(),
                            message: e.to_string(),
                        })?;
                    schema::structure_sqlite(&db, &table, schema.as_deref())
                })
                .await
                .map_err(|e| CatableError::Driver {
                    driver: "sqlite".into(),
                    message: format!("task failed: {e}"),
                })?
            }
            _ => Err(CatableError::Driver {
                driver: "schema".into(),
                message: "Table structure is not available for this database type yet.".into(),
            }),
        }
    }

    /// The SQL `apply_changes` would run, without touching the database.
    pub fn preview_changes(&self, set: &RowChangeSet) -> Result<Vec<String>, CatableError> {
        mutation::preview(self.driver, set)
    }

    /// Applies a staged change set in ONE transaction. Each statement is checked by the guardrail
    /// (read-only connections are refused), and every update/delete must hit exactly one row;
    /// otherwise the whole set is rolled back and nothing is saved.
    pub async fn apply_changes(&self, set: &RowChangeSet) -> Result<ApplyResult, CatableError> {
        let plan = mutation::build_plan(self.driver, set)?;

        for statement in &plan {
            let verdict = GuardrailEngine::evaluate(&statement.sql, &self.config);
            if verdict.risk == QueryRisk::Blocked {
                return Err(CatableError::Driver {
                    driver: "edit".into(),
                    message: format!(
                        "Blocked by Guardrail: {}",
                        verdict.reason.unwrap_or_default()
                    ),
                });
            }
        }

        let rows_affected = match self.driver {
            DriverKind::Postgres => {
                let mut lease = self.lease_pg().await?;
                let outcome = apply_postgres(lease.client(), &plan).await;
                if matches!(&outcome, Err(e) if connection_lost_msg(&e.to_string())) {
                    lease.discard();
                }
                outcome?
            }
            DriverKind::Mysql | DriverKind::MariaDb => {
                let mut lease = self.lease_mysql().await?;
                let outcome = apply_mysql(lease.client(), &plan).await;
                if matches!(&outcome, Err(e) if connection_lost_msg(&e.to_string())) {
                    lease.discard();
                }
                outcome?
            }
            DriverKind::Sqlite => {
                let cfg = self.config.clone();
                let plan = plan.clone();
                tokio::task::spawn_blocking(move || apply_sqlite(&cfg, &plan))
                    .await
                    .map_err(|e| CatableError::Driver {
                        driver: "sqlite".into(),
                        message: format!("task failed: {e}"),
                    })??
            }
            _ => {
                return Err(CatableError::Driver {
                    driver: "edit".into(),
                    message: "Editing rows is not supported for this database type.".into(),
                });
            }
        };

        Ok(ApplyResult {
            statements: plan.len(),
            rows_affected,
            sql: plan.into_iter().map(|p| p.sql).collect(),
        })
    }

    /// Round-trip time of a trivial query, in milliseconds.
    pub async fn ping(&self) -> Result<u64, CatableError> {
        let start = Instant::now();
        let res = self.execute("SELECT 1").await;
        match res.error {
            Some(message) => Err(CatableError::Driver {
                driver: format!("{:?}", self.driver).to_lowercase(),
                message,
            }),
            None => Ok(start.elapsed().as_millis() as u64),
        }
    }
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SessionManager {
    sessions: RwLock<HashMap<String, Arc<Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a session. Connecting again replaces any session for the same
    /// saved connection, so edited settings always take effect.
    pub async fn open(&self, cfg: ConnectionConfig) -> Result<SessionInfo, CatableError> {
        self.close_connection(&cfg.id).await;
        let session = Session::open(cfg).await?;
        let info = session.info();
        self.sessions
            .write()
            .await
            .insert(info.session_id.clone(), session);
        Ok(info)
    }

    pub async fn get(&self, session_id: &str) -> Result<Arc<Session>, CatableError> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .cloned()
            .ok_or_else(|| CatableError::Driver {
                driver: "session".into(),
                message: format!(
                    "Session '{session_id}' is not open (it may have been closed). Connect again."
                ),
            })
    }

    pub async fn close(&self, session_id: &str) -> bool {
        self.sessions.write().await.remove(session_id).is_some()
    }

    async fn close_connection(&self, connection_id: &str) {
        self.sessions
            .write()
            .await
            .retain(|_, session| session.connection_id != connection_id);
    }

    pub async fn close_all(&self) {
        self.sessions.write().await.clear();
    }

    pub async fn count(&self) -> usize {
        self.sessions.read().await.len()
    }
}

// ---------------------------------------------------------------------------
// Tests (no external servers: pool with dummy clients + a local SQLite file)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::Environment;
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn make_counted(counter: &Arc<AtomicUsize>) -> Result<usize, CatableError> {
        Ok(counter.fetch_add(1, Ordering::SeqCst))
    }

    #[tokio::test]
    async fn pool_reuses_idle_clients_instead_of_reconnecting() {
        let pool: Arc<Pool<usize>> = Pool::new(4);
        let made = Arc::new(AtomicUsize::new(0));

        for _ in 0..50 {
            let lease = pool.lease(|_| true, || make_counted(&made)).await.unwrap();
            drop(lease);
        }

        assert_eq!(
            made.load(Ordering::SeqCst),
            1,
            "50 sequential leases must open 1 client"
        );
        assert_eq!(pool.idle_len(), 1);
    }

    #[tokio::test]
    async fn pool_never_hands_out_more_than_its_size() {
        let pool: Arc<Pool<usize>> = Pool::new(2);
        let made = Arc::new(AtomicUsize::new(0));

        let a = pool.lease(|_| true, || make_counted(&made)).await.unwrap();
        let b = pool.lease(|_| true, || make_counted(&made)).await.unwrap();

        // A third lease must wait until one is returned.
        let third = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            pool.lease(|_| true, || make_counted(&made)),
        )
        .await;
        assert!(third.is_err(), "third lease should block while 2 are out");

        drop(a);
        let c = pool.lease(|_| true, || make_counted(&made)).await.unwrap();
        drop(b);
        drop(c);
        assert_eq!(
            made.load(Ordering::SeqCst),
            2,
            "no more than pool-size clients were created"
        );
    }

    #[tokio::test]
    async fn discarded_and_unusable_clients_are_not_reused() {
        let pool: Arc<Pool<usize>> = Pool::new(4);
        let made = Arc::new(AtomicUsize::new(0));

        let mut lease = pool.lease(|_| true, || make_counted(&made)).await.unwrap();
        lease.discard();
        drop(lease);
        assert_eq!(
            pool.idle_len(),
            0,
            "a discarded client must not return to the pool"
        );

        // A client that fails the liveness check is skipped and replaced.
        let lease = pool.lease(|_| true, || make_counted(&made)).await.unwrap();
        drop(lease);
        let lease = pool.lease(|_| false, || make_counted(&made)).await.unwrap();
        drop(lease);
        assert_eq!(made.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn detects_lost_connections_but_not_query_errors() {
        assert!(connection_lost_msg("MySQL server has gone away"));
        assert!(connection_lost_msg("db error: connection closed"));
        assert!(connection_lost_msg("I/O error: Broken pipe (os error 32)"));
        assert!(connection_lost_msg("I/O error: unexpected end of file"));
        assert!(connection_lost_msg(
            "Lost connection to MySQL server during query"
        ));
        assert!(connection_lost_msg(
            "ERROR 1927 (70100): Connection was killed"
        ));
        assert!(!connection_lost_msg("syntax error at or near \"SELEC\""));
        assert!(!connection_lost_msg(
            "duplicate key value violates unique constraint"
        ));
    }

    #[test]
    fn only_reads_are_retried() {
        assert!(is_read_statement("  select 1"));
        assert!(is_read_statement("/* hi */ SELECT * FROM t"));
        assert!(is_read_statement("EXPLAIN SELECT 1"));
        assert!(!is_read_statement("DELETE FROM t WHERE id = 1"));
        assert!(!is_read_statement("INSERT INTO t VALUES (1)"));
    }

    fn sqlite_config(path: &std::path::Path) -> ConnectionConfig {
        let mut cfg = ConnectionConfig::sandbox();
        cfg.id = format!("it-{}", uuid::Uuid::new_v4());
        cfg.name = "session test".into();
        cfg.environment = Environment::Development;
        cfg.database = path.to_string_lossy().into_owned();
        cfg
    }

    #[tokio::test]
    async fn sqlite_session_executes_lists_and_closes() {
        let path =
            std::env::temp_dir().join(format!("catable-session-{}.db", uuid::Uuid::new_v4()));
        let manager = SessionManager::new();

        let info = manager
            .open(sqlite_config(&path))
            .await
            .expect("open sqlite session");
        assert_eq!(manager.count().await, 1);
        assert!(!info.tunneled);

        let session = manager.get(&info.session_id).await.unwrap();

        let res = session.execute("SELECT count(*) AS n FROM users").await;
        assert!(res.error.is_none(), "unexpected error: {:?}", res.error);
        assert_eq!(
            res.rows[0][0],
            serde_json::json!(5),
            "sample data has 5 users"
        );

        let tables = session.list_tables().await.unwrap();
        assert!(tables.iter().any(|t| t.name == "users"));

        assert!(session.ping().await.is_ok());

        // The guardrail runs inside the session too.
        let blocked_cfg = {
            let mut c = sqlite_config(&path);
            c.read_only = true;
            c
        };
        let ro = manager.open(blocked_cfg).await.unwrap();
        let ro_session = manager.get(&ro.session_id).await.unwrap();
        let denied = ro_session.execute("DELETE FROM users WHERE id = 1").await;
        assert!(denied.error.as_deref().unwrap_or("").contains("Blocked"));

        assert!(manager.close(&ro.session_id).await);
        let gone = manager.get(&ro.session_id).await;
        assert!(gone.is_err(), "closed session must not be reachable");

        manager.close_all().await;
        assert_eq!(manager.count().await, 0);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn browse_filters_sorts_pages_and_counts_server_side() {
        use crate::sql_builder::{Filter, FilterMode, FilterOp, OrderBy};

        let path =
            std::env::temp_dir().join(format!("catable-session-{}.db", uuid::Uuid::new_v4()));
        let manager = SessionManager::new();
        let info = manager.open(sqlite_config(&path)).await.unwrap();
        let session = manager.get(&info.session_id).await.unwrap();

        let mut req = BrowseRequest {
            table: "users".into(),
            schema: None,
            limit: 2,
            offset: 0,
            order_by: vec![OrderBy {
                column: "id".into(),
                descending: true,
            }],
            filters: vec![],
            filter_mode: FilterMode::And,
            search: None,
            search_columns: vec![],
            with_total: true,
        };

        let page = session.browse(&req).await.unwrap();
        assert!(page.result.error.is_none(), "{:?}", page.result.error);
        assert_eq!(page.result.rows.len(), 2, "limit applies server-side");
        assert_eq!(page.total, Some(5), "total is the exact row count");
        assert_eq!(
            page.result.rows[0][0],
            serde_json::json!(5),
            "sorted by id DESC"
        );

        req.offset = 4;
        assert_eq!(
            session.browse(&req).await.unwrap().result.rows.len(),
            1,
            "last page"
        );

        // Filter: the total follows the filter, not the whole table.
        req.offset = 0;
        req.filters = vec![Filter {
            column: "role".into(),
            op: FilterOp::Eq,
            value: Some("admin".into()),
            values: vec![],
        }];
        let admins = session.browse(&req).await.unwrap();
        assert_eq!(admins.total, Some(1));
        assert_eq!(admins.result.rows.len(), 1);

        // An injection-shaped value matches nothing and does not break the query.
        req.filters[0].value = Some("x' OR '1'='1".into());
        let none = session.browse(&req).await.unwrap();
        assert!(none.result.error.is_none());
        assert_eq!(none.total, Some(0));

        assert!(
            session
                .build_select_sql(&req)
                .unwrap()
                .starts_with("SELECT * FROM \"users\"")
        );

        manager.close_all().await;
        let _ = std::fs::remove_file(&path);
    }

    fn text(v: &str) -> crate::mutation::CellValue {
        crate::mutation::CellValue::Text(v.into())
    }

    fn key(id: &str) -> Vec<crate::mutation::KeyPart> {
        vec![crate::mutation::KeyPart {
            column: "id".into(),
            value: text(id),
        }]
    }

    fn rename(id: &str, to: &str, original: Option<&str>) -> crate::mutation::RowUpdate {
        crate::mutation::RowUpdate {
            key: key(id),
            changes: vec![crate::mutation::ColumnChange {
                column: "name".into(),
                value: text(to),
                original: original.map(text),
            }],
        }
    }

    async fn names(session: &Session) -> Vec<String> {
        let res = session.execute("SELECT name FROM edit_t ORDER BY id").await;
        assert!(res.error.is_none(), "{:?}", res.error);
        res.rows
            .iter()
            .map(|r| r[0].as_str().unwrap_or("<null>").to_string())
            .collect()
    }

    #[tokio::test]
    async fn row_edits_commit_atomically_and_roll_back_on_any_failure() {
        use crate::mutation::{ColumnValue, RowChangeSet, RowDelete, RowInsert};

        let path =
            std::env::temp_dir().join(format!("catable-session-{}.db", uuid::Uuid::new_v4()));
        let manager = SessionManager::new();
        let info = manager.open(sqlite_config(&path)).await.unwrap();
        let session = manager.get(&info.session_id).await.unwrap();

        for sql in [
            "CREATE TABLE edit_t (id INTEGER PRIMARY KEY, name TEXT, note TEXT)",
            "INSERT INTO edit_t VALUES (1, 'a', NULL), (2, 'b', ''), (3, 'c', 'x')",
        ] {
            assert!(session.execute(sql).await.error.is_none(), "{sql}");
        }

        // 1) A mixed set commits as one unit.
        let mut set = RowChangeSet {
            table: "edit_t".into(),
            ..Default::default()
        };
        set.updates.push(rename("1", "A1", Some("a")));
        set.inserts.push(RowInsert {
            values: vec![
                ColumnValue {
                    column: "id".into(),
                    value: text("4"),
                },
                ColumnValue {
                    column: "name".into(),
                    value: text("d"),
                },
            ],
        });
        set.deletes.push(RowDelete { key: key("3") });
        let preview = session.preview_changes(&set).unwrap();
        assert_eq!(preview.len(), 3);
        let done = session.apply_changes(&set).await.unwrap();
        assert_eq!(done.statements, 3);
        assert_eq!(names(&session).await, vec!["A1", "b", "d"]);

        // 2) A failing statement anywhere rolls back everything before it.
        let mut set = RowChangeSet {
            table: "edit_t".into(),
            ..Default::default()
        };
        set.updates.push(rename("1", "SHOULD-NOT-STICK", None));
        set.inserts.push(RowInsert {
            values: vec![ColumnValue {
                column: "id".into(),
                value: text("2"),
            }], // duplicate PK
        });
        let err = session.apply_changes(&set).await.unwrap_err().to_string();
        assert!(err.contains("Nothing was saved"), "{err}");
        assert_eq!(
            names(&session).await,
            vec!["A1", "b", "d"],
            "the update must have been rolled back"
        );

        // 3) Editing on top of someone else's change is refused (optimistic concurrency).
        let mut set = RowChangeSet {
            table: "edit_t".into(),
            ..Default::default()
        };
        set.updates.push(rename("1", "Q", Some("stale-value")));
        let err = session.apply_changes(&set).await.unwrap_err().to_string();
        assert!(err.contains("affected 0 rows instead of 1"), "{err}");
        assert_eq!(names(&session).await, vec!["A1", "b", "d"]);

        // 4) Writing the same value is not a conflict.
        let mut set = RowChangeSet {
            table: "edit_t".into(),
            ..Default::default()
        };
        set.updates.push(rename("1", "A1", None));
        assert!(session.apply_changes(&set).await.is_ok());

        manager.close_all().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_non_unique_key_is_refused_instead_of_editing_many_rows() {
        use crate::mutation::RowChangeSet;

        let path =
            std::env::temp_dir().join(format!("catable-session-{}.db", uuid::Uuid::new_v4()));
        let manager = SessionManager::new();
        let info = manager.open(sqlite_config(&path)).await.unwrap();
        let session = manager.get(&info.session_id).await.unwrap();

        // No primary key and a duplicated "key" value.
        for sql in [
            "CREATE TABLE edit_t (id INTEGER, name TEXT, note TEXT)",
            "INSERT INTO edit_t VALUES (1, 'a', NULL), (1, 'b', NULL)",
        ] {
            assert!(session.execute(sql).await.error.is_none(), "{sql}");
        }
        let mut set = RowChangeSet {
            table: "edit_t".into(),
            ..Default::default()
        };
        set.updates.push(rename("1", "oops", None));
        let err = session.apply_changes(&set).await.unwrap_err().to_string();
        assert!(err.contains("affected 2 rows instead of 1"), "{err}");
        assert_eq!(
            names(&session).await,
            vec!["a", "b"],
            "neither row may change"
        );

        manager.close_all().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn read_only_connections_cannot_apply_edits() {
        use crate::mutation::RowChangeSet;

        let path =
            std::env::temp_dir().join(format!("catable-session-{}.db", uuid::Uuid::new_v4()));
        let manager = SessionManager::new();
        let mut cfg = sqlite_config(&path);
        let rw = manager.open(cfg.clone()).await.unwrap();
        let rw_session = manager.get(&rw.session_id).await.unwrap();
        for sql in [
            "CREATE TABLE edit_t (id INTEGER PRIMARY KEY, name TEXT, note TEXT)",
            "INSERT INTO edit_t VALUES (1, 'a', NULL)",
        ] {
            assert!(rw_session.execute(sql).await.error.is_none());
        }

        cfg.read_only = true;
        let ro = manager.open(cfg).await.unwrap();
        let ro_session = manager.get(&ro.session_id).await.unwrap();
        let mut set = RowChangeSet {
            table: "edit_t".into(),
            ..Default::default()
        };
        set.updates.push(rename("1", "nope", None));
        let err = ro_session
            .apply_changes(&set)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("Blocked by Guardrail"), "{err}");
        assert_eq!(names(&ro_session).await, vec!["a"]);

        manager.close_all().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn reconnecting_replaces_the_previous_session_for_a_connection() {
        let path =
            std::env::temp_dir().join(format!("catable-session-{}.db", uuid::Uuid::new_v4()));
        let manager = SessionManager::new();
        let cfg = sqlite_config(&path);

        let first = manager.open(cfg.clone()).await.unwrap();
        let second = manager.open(cfg).await.unwrap();

        assert_ne!(first.session_id, second.session_id);
        assert_eq!(manager.count().await, 1);
        assert!(manager.get(&first.session_id).await.is_err());
        assert!(manager.get(&second.session_id).await.is_ok());

        manager.close_all().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn opening_an_unreachable_server_fails_fast_with_a_clear_error() {
        let mut cfg = ConnectionConfig::sandbox();
        cfg.id = "it-unreachable".into();
        cfg.driver = DriverKind::Postgres;
        cfg.host = "127.0.0.1".into();
        cfg.port = 1; // nothing listens here
        cfg.database = "x".into();
        cfg.connect_timeout_secs = Some(2);

        let started = Instant::now();
        let err = SessionManager::new()
            .open(cfg)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            started.elapsed().as_secs() < 5,
            "must respect the 2s connect timeout"
        );
        assert!(!err.is_empty());
    }
}
