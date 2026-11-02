//! Checks shared by every SQL engine: connect, seed shape, query path and
//! guardrails. Each check runs once per engine.

use catable_core::{ConnectionConfig, DriverKind, Environment, QueryExecutor, QueryResult};
use libtest_mimic::Trial;
use serde_json::{Value, json};

use crate::env::Target;
use crate::{Check, trial};

/// Row counts every seed in dev/seed/ must produce.
const SEED_ROWS: &[(&str, u64)] = &[
    ("users", 5),
    ("orders", 8),
    ("products", 4),
    ("events", 100_000),
    ("Mixed Case Table", 4),
    ("audit_log", 3),
];

/// `networked` adds checks that only make sense for client/server engines.
pub fn trials(engine: &str, target: &Target<ConnectionConfig>, networked: bool) -> Vec<Trial> {
    let mut trials = vec![
        trial(format!("{engine}::connect"), target, connect),
        trial(
            format!("{engine}::seed_tables_present"),
            target,
            seed_tables_present,
        ),
        trial(
            format!("{engine}::seed_row_counts"),
            target,
            seed_row_counts,
        ),
        trial(
            format!("{engine}::seed_null_vs_empty"),
            target,
            seed_null_vs_empty,
        ),
        trial(
            format!("{engine}::select_users_ordered"),
            target,
            select_users_ordered,
        ),
        trial(
            format!("{engine}::insert_reports_rows_affected"),
            target,
            insert_reports_rows_affected,
        ),
        trial(
            format!("{engine}::read_only_blocks_write"),
            target,
            read_only_blocks_write,
        ),
        trial(
            format!("{engine}::production_blocks_unbounded_delete"),
            target,
            production_blocks_unbounded_delete,
        ),
    ];
    if networked {
        trials.push(trial(
            format!("{engine}::wrong_password_rejected"),
            target,
            wrong_password_rejected,
        ));
    }
    trials
}

/// Hand-rolled identifier quoting for test SQL only.
pub fn quote(driver: DriverKind, name: &str) -> String {
    match driver {
        DriverKind::Mysql | DriverKind::MariaDb => format!("`{}`", name.replace('`', "``")),
        _ => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

pub async fn exec(conn: &ConnectionConfig, sql: &str) -> Result<QueryResult, String> {
    let res = QueryExecutor::execute_query_async(conn, sql).await;
    match res.error {
        Some(e) => Err(format!("{sql}: {e}")),
        None => Ok(res),
    }
}

pub async fn scalar_u64(conn: &ConnectionConfig, sql: &str) -> Result<u64, String> {
    let res = exec(conn, sql).await?;
    let value = res
        .rows
        .first()
        .and_then(|r| r.first())
        .ok_or_else(|| format!("{sql}: no rows"))?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .ok_or_else(|| format!("{sql}: not a count: {value}"))
}

async fn connect(conn: ConnectionConfig) -> Check {
    let res = QueryExecutor::test_connection_async(&conn).await;
    ensure!(res.success, "test_connection failed: {}", res.message);

    // The driver labels MySQL/MariaDB from the profile, so ask the server.
    let sql = match conn.driver {
        DriverKind::Sqlite => "SELECT sqlite_version()",
        _ => "SELECT version()",
    };
    let version = exec(&conn, sql).await?.rows[0][0]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let is_expected = match conn.driver {
        DriverKind::Postgres => version.starts_with("PostgreSQL"),
        DriverKind::MariaDb => version.contains("MariaDB"),
        DriverKind::Mysql => !version.is_empty() && !version.contains("MariaDB"),
        _ => version.starts_with('3'),
    };
    ensure!(
        is_expected,
        "{:?} target reports server version {version:?}",
        conn.driver
    );
    Ok(())
}

async fn wrong_password_rejected(mut conn: ConnectionConfig) -> Check {
    conn.password = Some("definitely-not-the-password".into());
    let res = QueryExecutor::test_connection_async(&conn).await;
    ensure!(
        !res.success,
        "connected with a wrong password: {}",
        res.message
    );
    Ok(())
}

async fn seed_tables_present(conn: ConnectionConfig) -> Check {
    let tables = QueryExecutor::list_tables_async(&conn)
        .await
        .map_err(|e| e.to_string())?;
    let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    for (table, _) in SEED_ROWS {
        ensure!(
            names.contains(table),
            "table {table:?} missing; found {names:?}"
        );
    }
    Ok(())
}

async fn seed_row_counts(conn: ConnectionConfig) -> Check {
    for (table, want) in SEED_ROWS {
        let sql = format!("SELECT count(*) FROM {}", quote(conn.driver, table));
        let got = scalar_u64(&conn, &sql).await?;
        ensure!(got == *want, "{table}: expected {want} rows, got {got}");
    }
    Ok(())
}

async fn seed_null_vs_empty(conn: ConnectionConfig) -> Check {
    // note = NULL when n % 3 = 0, '' when n % 3 = 1, text otherwise (n = 1..=100 000).
    for (predicate, want) in [
        ("note IS NULL", 33_333),
        ("note = ''", 33_334),
        ("note <> ''", 33_333),
    ] {
        let got = scalar_u64(
            &conn,
            &format!("SELECT count(*) FROM events WHERE {predicate}"),
        )
        .await?;
        ensure!(
            got == want,
            "events WHERE {predicate}: expected {want}, got {got}"
        );
    }
    Ok(())
}

async fn select_users_ordered(conn: ConnectionConfig) -> Check {
    let res = exec(&conn, "SELECT id, name, email FROM users ORDER BY id").await?;
    ensure!(
        res.columns == ["id", "name", "email"],
        "columns: {:?}",
        res.columns
    );
    ensure!(
        res.rows.len() == 5,
        "expected 5 users, got {}",
        res.rows.len()
    );
    ensure!(
        res.rows[0][1] == json!("Ada Lovelace"),
        "first user: {}",
        res.rows[0][1]
    );
    ensure!(
        res.rows[2][1] == json!("Seán O'Brien"),
        "unicode/apostrophe round-trip: {}",
        res.rows[2][1]
    );
    // MySQL/MariaDB still return integers as strings (text protocol); typed
    // values are S1-D, so only engines that already type them are checked.
    if matches!(conn.driver, DriverKind::Postgres | DriverKind::Sqlite) {
        ensure!(
            matches!(res.rows[0][0], Value::Number(_)),
            "users.id should be numeric, got {}",
            res.rows[0][0]
        );
    }
    Ok(())
}

async fn insert_reports_rows_affected(conn: ConnectionConfig) -> Check {
    let email = format!("it-insert-{}@example.test", conn.id);
    let cleanup = format!("DELETE FROM users WHERE email = '{email}'");
    exec(&conn, &cleanup).await?;

    let insert = format!(
        "INSERT INTO users (name, email, role) VALUES ('IT Insert', '{email}', 'developer')"
    );
    let res = exec(&conn, &insert).await?;
    ensure!(
        res.rows_affected == 1,
        "insert rows_affected = {}",
        res.rows_affected
    );

    let res = exec(&conn, &cleanup).await?;
    ensure!(
        res.rows_affected == 1,
        "delete rows_affected = {}",
        res.rows_affected
    );
    Ok(())
}

async fn read_only_blocks_write(mut conn: ConnectionConfig) -> Check {
    conn.read_only = true;
    let res = QueryExecutor::execute_query_async(
        &conn,
        "INSERT INTO users (name, email, role) VALUES ('Should Fail', 'ro@example.test', 'dev')",
    )
    .await;
    let error = res.error.unwrap_or_default();
    ensure!(
        error.contains("Query Blocked by Guardrail"),
        "write on read-only connection: {error:?}"
    );
    Ok(())
}

async fn production_blocks_unbounded_delete(mut conn: ConnectionConfig) -> Check {
    conn.environment = Environment::Production;
    let res = QueryExecutor::execute_query_async(&conn, "DELETE FROM audit_log").await;
    let error = res.error.unwrap_or_default();
    ensure!(
        error.contains("Query Blocked by Guardrail"),
        "unbounded DELETE on production: {error:?}"
    );
    Ok(())
}
