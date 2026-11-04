//! Session checks against real servers: connection reuse, pool bound, recovery
//! after the server kills a connection, and server-side browse.
//!
//! Reuse is proven through the server's own connection id (`pg_backend_pid()`,
//! `CONNECTION_ID()`), not through counters the test could misread. A control
//! (the per-call executor, which reconnects every time) shows the check can fail.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use catable_core::session::DEFAULT_POOL_SIZE;
use catable_core::{
    BrowseRequest, CellValue, ColumnChange, ColumnValue, ConnectionConfig, DriverKind, Filter,
    FilterMode, FilterOp, KeyPart, OrderBy, RowChangeSet, RowDelete, RowInsert, RowUpdate, Session,
    SessionManager,
};
use libtest_mimic::Trial;

use crate::engines::{exec, scalar_u64};
use crate::env::Target;
use crate::{Check, trial};

/// `networked` adds the checks that need a client/server engine.
pub fn trials(engine: &str, target: &Target<ConnectionConfig>, networked: bool) -> Vec<Trial> {
    let mut trials = vec![
        trial(
            format!("{engine}::session_browse_matches_seed"),
            target,
            browse_matches_seed,
        ),
        trial(
            format!("{engine}::session_browse_values_are_not_sql"),
            target,
            browse_values_are_not_sql,
        ),
        trial(
            format!("{engine}::session_edits_commit_and_roll_back"),
            target,
            edits_commit_and_roll_back,
        ),
        trial(
            format!("{engine}::session_structure_matches_seed"),
            target,
            structure_matches_seed,
        ),
    ];
    if networked {
        trials.push(trial(
            format!("{engine}::session_reuses_one_connection"),
            target,
            reuses_one_connection,
        ));
        trials.push(trial(
            format!("{engine}::session_pool_is_bounded_and_parallel"),
            target,
            pool_is_bounded_and_parallel,
        ));
        trials.push(trial(
            format!("{engine}::session_recovers_after_server_kill"),
            target,
            recovers_after_server_kill,
        ));
    }
    trials
}

async fn open(cfg: &ConnectionConfig) -> Result<Arc<Session>, String> {
    let manager = SessionManager::new();
    let info = manager.open(cfg.clone()).await.map_err(|e| e.to_string())?;
    manager
        .get(&info.session_id)
        .await
        .map_err(|e| e.to_string())
}

fn backend_id_sql(driver: DriverKind) -> &'static str {
    match driver {
        DriverKind::Postgres => "SELECT pg_backend_pid()",
        _ => "SELECT CONNECTION_ID()",
    }
}

/// Same id, but the query takes ~0.3s so concurrent calls really overlap.
fn slow_backend_id_sql(driver: DriverKind) -> &'static str {
    match driver {
        DriverKind::Postgres => "SELECT pg_backend_pid() FROM (SELECT pg_sleep(0.3)) s",
        _ => "SELECT CONNECTION_ID() FROM (SELECT SLEEP(0.3)) s",
    }
}

fn first_u64(res: &catable_core::QueryResult, sql: &str) -> Result<u64, String> {
    if let Some(e) = &res.error {
        return Err(format!("{sql}: {e}"));
    }
    let v = res
        .rows
        .first()
        .and_then(|r| r.first())
        .ok_or_else(|| format!("{sql}: no rows"))?;
    v.as_u64()
        .or_else(|| v.as_i64().map(|n| n as u64))
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .ok_or_else(|| format!("{sql}: not an id: {v}"))
}

async fn backend_id(session: &Session, sql: &str) -> Result<u64, String> {
    first_u64(&session.execute(sql).await, sql)
}

async fn reuses_one_connection(cfg: ConnectionConfig) -> Check {
    let session = open(&cfg).await?;
    let sql = backend_id_sql(cfg.driver);

    let mut pooled = HashSet::new();
    for _ in 0..100 {
        pooled.insert(backend_id(&session, sql).await?);
    }
    ensure!(
        pooled.len() == 1,
        "100 sequential queries on one session used {} server connections: {pooled:?}",
        pooled.len()
    );

    // Control: the per-call executor reconnects each time, so this check can tell the difference.
    let mut per_call = HashSet::new();
    for _ in 0..5 {
        per_call.insert(scalar_u64(&cfg, sql).await?);
    }
    ensure!(
        per_call.len() > 1,
        "control failed: per-call executor reused a connection ({per_call:?}), so this test proves nothing"
    );
    Ok(())
}

async fn pool_is_bounded_and_parallel(cfg: ConnectionConfig) -> Check {
    let session = open(&cfg).await?;
    let sql = slow_backend_id_sql(cfg.driver);

    let mut handles = Vec::new();
    for _ in 0..(DEFAULT_POOL_SIZE * 4) {
        let s = Arc::clone(&session);
        handles.push(tokio::spawn(async move { (s.execute(sql).await, sql) }));
    }

    let mut ids = HashSet::new();
    for handle in handles {
        let (res, sql) = handle.await.map_err(|e| e.to_string())?;
        ids.insert(first_u64(&res, sql)?);
    }
    ensure!(
        ids.len() <= DEFAULT_POOL_SIZE,
        "{} concurrent queries used {} server connections, pool size is {DEFAULT_POOL_SIZE}: {ids:?}",
        DEFAULT_POOL_SIZE * 4,
        ids.len()
    );
    ensure!(
        ids.len() > 1,
        "concurrent queries all ran on one connection ({ids:?}); the pool is not parallel"
    );
    Ok(())
}

async fn recovers_after_server_kill(cfg: ConnectionConfig) -> Check {
    let session = open(&cfg).await?;
    let sql = backend_id_sql(cfg.driver);
    let before = backend_id(&session, sql).await?;

    let kill = match cfg.driver {
        DriverKind::Postgres => format!("SELECT pg_terminate_backend({before})"),
        _ => format!("KILL {before}"),
    };
    exec(&cfg, &kill).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // A read on a dead pooled connection must be retried once on a fresh one.
    let after = backend_id(&session, sql).await.map_err(|e| {
        format!("session did not recover after its server connection was killed: {e}")
    })?;
    ensure!(
        after != before,
        "still on killed connection {before}; the dead client was not replaced"
    );
    Ok(())
}

fn request(table: &str) -> BrowseRequest {
    BrowseRequest {
        table: table.into(),
        schema: None,
        limit: 50,
        offset: 0,
        order_by: vec![],
        filters: vec![],
        filter_mode: FilterMode::And,
        search: None,
        search_columns: vec![],
        with_total: true,
    }
}

fn filter(column: &str, op: FilterOp, value: &str) -> Filter {
    Filter {
        column: column.into(),
        op,
        value: Some(value.into()),
        values: vec![],
    }
}

async fn browse(
    session: &Session,
    req: &BrowseRequest,
) -> Result<catable_core::BrowseResult, String> {
    let page = session.browse(req).await.map_err(|e| e.to_string())?;
    if let Some(e) = &page.result.error {
        return Err(format!("{}: {e}", page.sql));
    }
    Ok(page)
}

async fn browse_matches_seed(cfg: ConnectionConfig) -> Check {
    let session = open(&cfg).await?;

    // 100 000 rows: paged and counted on the server, last page is short.
    let mut req = request("events");
    req.offset = 99_990;
    req.order_by = vec![OrderBy {
        column: "id".into(),
        descending: false,
    }];
    let page = browse(&session, &req).await?;
    ensure!(
        page.total == Some(100_000),
        "events total: {:?}",
        page.total
    );
    ensure!(
        page.result.rows.len() == 10,
        "last page has {} rows, expected 10",
        page.result.rows.len()
    );

    // Names with spaces, capitals, quotes and backticks need per-driver quoting.
    let mut req = request("Mixed Case Table");
    req.order_by = vec![OrderBy {
        column: "Id".into(),
        descending: false,
    }];
    let all = browse(&session, &req).await?;
    ensure!(all.total == Some(4), "mixed total: {:?}", all.total);
    ensure!(
        all.result.rows[0][1].as_str() == Some("Alpha"),
        "first display name: {:?}",
        all.result.rows[0][1]
    );

    req.filters = vec![filter("Display Name", FilterOp::Eq, "Beta \"quoted\"")];
    ensure!(
        browse(&session, &req).await?.total == Some(1),
        "eq with double quotes"
    );

    req.filters = vec![filter("Display Name", FilterOp::Contains, "tick")];
    ensure!(
        browse(&session, &req).await?.total == Some(1),
        "contains with backticks"
    );

    // Quick search: any column, case-insensitive, numbers are cast to text, '%' is literal.
    req.filters.clear();
    req.search_columns = vec!["Display Name".into(), "Id".into()];
    req.search = Some("ALP".into());
    ensure!(
        browse(&session, &req).await?.total == Some(1),
        "case-insensitive search"
    );
    req.search = Some("3".into());
    ensure!(
        browse(&session, &req).await?.total == Some(1),
        "search in a numeric column"
    );
    req.search = Some("%".into());
    ensure!(
        browse(&session, &req).await?.total == Some(0),
        "a % in the search must be literal"
    );

    // Sorting happens on the server.
    req.search = None;
    req.order_by = vec![OrderBy {
        column: "Id".into(),
        descending: true,
    }];
    let desc = browse(&session, &req).await?;
    let first_id = desc.result.rows[0][0].to_string();
    ensure!(
        first_id.trim_matches('"') == "4",
        "descending sort, first id: {first_id}"
    );
    Ok(())
}

async fn browse_values_are_not_sql(cfg: ConnectionConfig) -> Check {
    let session = open(&cfg).await?;
    let mut req = request("users");

    // The payload from the plan matches nothing and does not break the query.
    req.filters = vec![filter("name", FilterOp::Eq, "x' OR '1'='1")];
    let none = browse(&session, &req).await?;
    ensure!(
        none.total == Some(0),
        "injection payload matched rows: {:?}",
        none.total
    );

    // An apostrophe in real data round-trips: "Seán O'Brien" is seeded.
    req.filters = vec![filter("name", FilterOp::Eq, "Seán O'Brien")];
    ensure!(
        browse(&session, &req).await?.total == Some(1),
        "a value with an apostrophe did not match its row"
    );

    // A trailing backslash must not escape the closing quote (MySQL default mode).
    req.filters = vec![filter("name", FilterOp::Eq, "x\\")];
    let slash = browse(&session, &req).await?;
    ensure!(slash.total == Some(0), "backslash value: {:?}", slash.total);
    Ok(())
}

fn tk(id: &str) -> Vec<KeyPart> {
    vec![KeyPart {
        column: "id".into(),
        value: CellValue::Text(id.into()),
    }]
}

fn rename(id: &str, to: &str, original: Option<CellValue>) -> RowUpdate {
    RowUpdate {
        key: tk(id),
        changes: vec![ColumnChange {
            column: "name".into(),
            value: CellValue::Text(to.into()),
            original,
        }],
    }
}

async fn names(cfg: &ConnectionConfig, table: &str) -> Result<Vec<String>, String> {
    let sql = format!("SELECT COALESCE(name, '<null>') FROM {table} ORDER BY id");
    let res = exec(cfg, &sql).await?;
    Ok(res
        .rows
        .iter()
        .map(|r| r[0].as_str().unwrap_or("<non-text>").to_string())
        .collect())
}

/// Row edits against a scratch table: an atomic commit, a full rollback when one statement
/// fails, a refusal when the row changed underneath the user, NULL vs '' round trip, and the
/// case MySQL gets wrong by default (writing the same value must not count as "0 rows").
async fn edits_commit_and_roll_back(cfg: ConnectionConfig) -> Check {
    let table = format!(
        "it_edit_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );
    let ddl = match cfg.driver {
        DriverKind::Mysql | DriverKind::MariaDb => format!(
            "CREATE TABLE {table} (id INT PRIMARY KEY, name VARCHAR(100), note VARCHAR(100))"
        ),
        _ => format!("CREATE TABLE {table} (id INTEGER PRIMARY KEY, name TEXT, note TEXT)"),
    };
    exec(&cfg, &ddl).await?;
    let outcome = edit_scenarios(&cfg, &table).await;
    let _ = exec(&cfg, &format!("DROP TABLE {table}")).await; // never leave scratch tables behind
    outcome
}

async fn edit_scenarios(cfg: &ConnectionConfig, table: &str) -> Check {
    exec(
        cfg,
        &format!("INSERT INTO {table} VALUES (1, 'a', NULL), (2, 'b', ''), (3, 'c', 'x')"),
    )
    .await?;
    let session = open(cfg).await?;

    // 1) Mixed set commits as one unit; NULL and '' stay distinct.
    let mut set = RowChangeSet {
        table: table.into(),
        ..Default::default()
    };
    set.updates
        .push(rename("1", "A1", Some(CellValue::Text("a".into()))));
    set.updates.push(RowUpdate {
        key: tk("2"),
        changes: vec![ColumnChange {
            column: "note".into(),
            value: CellValue::Null,
            original: Some(CellValue::Text(String::new())),
        }],
    });
    set.inserts.push(RowInsert {
        values: vec![
            ColumnValue {
                column: "id".into(),
                value: CellValue::Text("4".into()),
            },
            ColumnValue {
                column: "name".into(),
                value: CellValue::Text("d".into()),
            },
        ],
    });
    set.deletes.push(RowDelete { key: tk("3") });
    let done = session
        .apply_changes(&set)
        .await
        .map_err(|e| e.to_string())?;
    ensure!(done.statements == 4, "statements: {}", done.statements);
    ensure!(
        names(cfg, table).await? == vec!["A1", "b", "d"],
        "after commit: {:?}",
        names(cfg, table).await?
    );
    let null_notes = scalar_u64(
        cfg,
        &format!("SELECT COUNT(*) FROM {table} WHERE note IS NULL"),
    )
    .await?;
    ensure!(
        null_notes == 3,
        "rows with NULL note: {null_notes} (expected ids 1, 2, 4)"
    );

    // 2) One bad statement rolls back everything before it.
    let mut set = RowChangeSet {
        table: table.into(),
        ..Default::default()
    };
    set.updates.push(rename("1", "MUST-NOT-STICK", None));
    set.inserts.push(RowInsert {
        values: vec![ColumnValue {
            column: "id".into(),
            value: CellValue::Text("2".into()),
        }],
    });
    ensure!(
        session.apply_changes(&set).await.is_err(),
        "a duplicate key was accepted"
    );
    ensure!(
        names(cfg, table).await? == vec!["A1", "b", "d"],
        "the update survived a failed commit: {:?}",
        names(cfg, table).await?
    );

    // 3) Editing a row someone else changed is refused, not overwritten.
    let mut set = RowChangeSet {
        table: table.into(),
        ..Default::default()
    };
    set.updates
        .push(rename("1", "Q", Some(CellValue::Text("stale".into()))));
    let err = session
        .apply_changes(&set)
        .await
        .err()
        .map(|e| e.to_string());
    ensure!(
        err.as_deref()
            .is_some_and(|m| m.contains("affected 0 rows instead of 1")),
        "stale edit result: {err:?}"
    );
    ensure!(
        names(cfg, table).await?[0] == "A1",
        "stale edit changed the row"
    );

    // 4) Writing the value that is already there must succeed (MySQL counts changed rows only
    //    unless the client asks for matched rows).
    let mut set = RowChangeSet {
        table: table.into(),
        ..Default::default()
    };
    set.updates.push(rename("1", "A1", None));
    session
        .apply_changes(&set)
        .await
        .map_err(|e| format!("same-value update was treated as a conflict: {e}"))?;
    Ok(())
}

/// The same schema is seeded in all three engines, so one set of expectations must hold for each:
/// keys, defaults, foreign keys with their rules, indexes and a usable CREATE TABLE.
async fn structure_matches_seed(cfg: ConnectionConfig) -> Check {
    let session = open(&cfg).await?;
    let structure = |table: &'static str| {
        let session = Arc::clone(&session);
        async move {
            session
                .describe_structure(table, None)
                .await
                .map_err(|e| format!("{table}: {e}"))
        }
    };

    let users = structure("users").await?;
    let names: Vec<&str> = users.columns.iter().map(|c| c.name.as_str()).collect();
    ensure!(
        names == ["id", "name", "email", "role", "created_at"],
        "users columns: {names:?}"
    );
    let id = &users.columns[0];
    ensure!(id.is_pk, "users.id must be the primary key");
    ensure!(id.is_auto_increment, "users.id must report auto-increment");
    let role = users
        .columns
        .iter()
        .find(|c| c.name == "role")
        .ok_or("no role column")?;
    ensure!(
        role.default
            .as_deref()
            .is_some_and(|d| d.contains("member")),
        "users.role default: {:?}",
        role.default
    );
    ensure!(!role.nullable, "users.role is NOT NULL");
    ensure!(
        users
            .indexes
            .iter()
            .any(|i| i.unique && !i.primary && i.columns == ["email"]),
        "users must have a unique index on email: {:?}",
        users.indexes
    );

    let orders = structure("orders").await?;
    ensure!(
        orders.foreign_keys.len() == 1,
        "orders FKs: {:?}",
        orders.foreign_keys
    );
    let fk = &orders.foreign_keys[0];
    ensure!(fk.columns == ["user_id"], "fk columns: {:?}", fk.columns);
    ensure!(fk.ref_table == "users", "fk target: {}", fk.ref_table);
    ensure!(
        fk.ref_columns == ["id"],
        "fk target columns: {:?}",
        fk.ref_columns
    );
    ensure!(
        fk.on_delete
            .as_deref()
            .is_some_and(|r| r.eq_ignore_ascii_case("CASCADE")),
        "fk ON DELETE: {:?}",
        fk.on_delete
    );
    ensure!(
        orders
            .indexes
            .iter()
            .any(|i| i.columns == ["user_id"] && !i.primary),
        "orders must have an index on user_id: {:?}",
        orders.indexes
    );
    let ddl = orders.ddl.as_deref().ok_or("orders has no DDL")?;
    ensure!(
        ddl.to_lowercase().contains("create table") && ddl.contains("fk_orders_user"),
        "orders DDL: {ddl}"
    );

    // Names that need quoting, and a table without a primary key or indexes.
    let mixed = structure("Mixed Case Table").await?;
    let mixed_names: Vec<&str> = mixed.columns.iter().map(|c| c.name.as_str()).collect();
    ensure!(
        mixed_names == ["Id", "Display Name", "isActive"],
        "mixed columns: {mixed_names:?}"
    );
    let audit = structure("audit_log").await?;
    ensure!(
        !audit.columns.iter().any(|c| c.is_pk),
        "audit_log must have no primary key"
    );
    ensure!(
        audit.foreign_keys.is_empty(),
        "audit_log has no foreign keys"
    );

    ensure!(
        session
            .describe_structure("no_such_table", None)
            .await
            .is_err(),
        "a missing table must be an error"
    );
    Ok(())
}
