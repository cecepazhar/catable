//! Integration tests for real PostgreSQL and MySQL drivers.
//!
//! These tests are feature-gated behind the `live-db-tests` cargo feature so
//! that `cargo test` runs quickly without requiring database containers.
//! Enable with: `cargo test -p catable-core --features live-db-tests`
//!
//! Assumes PostgreSQL on 127.0.0.1:5433 (user: catable_user, pass: secret, db: catable_test)
//! and MySQL on 127.0.0.1:3307 (user: catable_user, pass: catable_pass, db: catable_test).

#![cfg(feature = "live-db-tests")]

use catable_core::{ConnectionConfig, DriverKind, Environment, QueryExecutor};

fn pg_conn(read_only: bool) -> ConnectionConfig {
    ConnectionConfig {
        id: "pg-live".into(),
        name: "PostgreSQL Live Test".into(),
        driver: DriverKind::Postgres,
        environment: Environment::Development,
        host: "127.0.0.1".into(),
        port: 5433,
        database: "catable_test".into(),
        user: "catable_user".into(),
        password: Some("secret".into()),
        read_only,
        ssh_tunnel: None,
    }
}

fn mysql_conn(read_only: bool) -> ConnectionConfig {
    ConnectionConfig {
        id: "mysql-live".into(),
        name: "MySQL Live Test".into(),
        driver: DriverKind::Mysql,
        environment: Environment::Development,
        host: "127.0.0.1".into(),
        port: 3307,
        database: "catable_test".into(),
        user: "catable_user".into(),
        password: Some("catable_pass".into()),
        read_only,
        ssh_tunnel: None,
    }
}

#[tokio::test]
async fn postgres_test_connection_real() {
    let res = QueryExecutor::test_connection_async(&pg_conn(false)).await;
    assert!(res.success, "pg test failed: {}", res.message);
    assert!(
        res.server_version.to_lowercase().contains("postgres"),
        "unexpected version: {}",
        res.server_version
    );
}

#[tokio::test]
async fn postgres_test_connection_bad_credentials() {
    let mut conn = pg_conn(false);
    conn.password = Some("wrong_password".into());
    let res = QueryExecutor::test_connection_async(&conn).await;
    assert!(!res.success);
}

#[tokio::test]
async fn postgres_list_tables() {
    let tables = QueryExecutor::list_tables_async(&pg_conn(false))
        .await
        .expect("pg list_tables failed");
    assert!(tables.iter().any(|t| t.name == "users"), "no users table");
    assert!(tables.iter().any(|t| t.name == "products"), "no products table");

    let users = tables.iter().find(|t| t.name == "users").unwrap();
    assert!(
        users.columns.iter().any(|c| c.name == "id" && c.is_pk),
        "users.id should be pk: {:?}",
        users.columns
    );
    assert!(users.row_count >= 3, "expected >=3 users, got {}", users.row_count);
}

#[tokio::test]
async fn postgres_select_query() {
    let res = QueryExecutor::execute_query_async(&pg_conn(false), "SELECT id, name, email FROM users ORDER BY id")
        .await;
    assert!(res.error.is_none(), "pg select failed: {:?}", res.error);
    assert_eq!(res.columns, vec!["id", "name", "email"]);
    assert_eq!(res.rows.len(), 3);
    assert!(res.rows[0][0].is_i64());
    assert_eq!(res.rows[0][1], serde_json::json!("Cecep Saeful Azhar"));
}

#[tokio::test]
async fn postgres_insert_returns_affected_rows() {
    let conn = pg_conn(false);
    let res = QueryExecutor::execute_query_async(
        &conn,
        "INSERT INTO users (name, email, role) VALUES ('Live Test User', 'live-test@fathforce.com', 'developer')",
    )
    .await;
    assert!(res.error.is_none(), "pg insert failed: {:?}", res.error);
    assert_eq!(res.rows_affected, 1);

    // Clean up
    let del = QueryExecutor::execute_query_async(
        &conn,
        "DELETE FROM users WHERE email = 'live-test@fathforce.com'",
    )
    .await;
    assert!(del.error.is_none(), "pg cleanup failed: {:?}", del.error);
    assert_eq!(del.rows_affected, 1);
}

#[tokio::test]
async fn postgres_read_only_blocks_writes_at_driver_level() {
    let conn = pg_conn(true);
    let res = QueryExecutor::execute_query_async(
        &conn,
        "INSERT INTO users (name, email, role) VALUES ('Should Fail', 'ro@fathforce.com', 'dev')",
    )
    .await;
    // The guardrail engine blocks non-read queries on read-only connections
    // before any network call is made.
    assert!(res.error.is_some());
    assert!(res.error.unwrap().contains("Query Blocked by Guardrail"));
}

#[tokio::test]
async fn postgres_prod_guardrails_block_destructive() {
    let mut conn = pg_conn(false);
    conn.environment = Environment::Production;
    let res = QueryExecutor::execute_query_async(&conn, "TRUNCATE TABLE users").await;
    assert!(res.error.is_some());
    assert!(res.error.unwrap().contains("Query Blocked by Guardrail"));

    let res2 = QueryExecutor::execute_query_async(&conn, "DELETE FROM users").await;
    assert!(res2.error.is_some());
    assert!(res2.error.unwrap().contains("Query Blocked by Guardrail"));
}

#[tokio::test]
async fn mysql_test_connection_real() {
    let res = QueryExecutor::test_connection_async(&mysql_conn(false)).await;
    assert!(res.success, "mysql test failed: {}", res.message);
    assert!(
        res.server_version.to_lowercase().contains("mysql")
            || res.server_version.to_lowercase().contains("mariadb"),
        "unexpected version: {}",
        res.server_version
    );
}

#[tokio::test]
async fn mysql_test_connection_bad_credentials() {
    let mut conn = mysql_conn(false);
    conn.password = Some("wrong_password".into());
    let res = QueryExecutor::test_connection_async(&conn).await;
    assert!(!res.success);
}

#[tokio::test]
async fn mysql_list_tables() {
    let tables = QueryExecutor::list_tables_async(&mysql_conn(false))
        .await
        .expect("mysql list_tables failed");
    assert!(tables.iter().any(|t| t.name == "users"), "no users table");
    assert!(tables.iter().any(|t| t.name == "products"), "no products table");

    let users = tables.iter().find(|t| t.name == "users").unwrap();
    assert!(
        users.columns.iter().any(|c| c.name == "id" && c.is_pk),
        "users.id should be pk: {:?}",
        users.columns
    );
}

#[tokio::test]
async fn mysql_select_query() {
    let res = QueryExecutor::execute_query_async(&mysql_conn(false), "SELECT id, name, email FROM users ORDER BY id")
        .await;
    assert!(res.error.is_none(), "mysql select failed: {:?}", res.error);
    assert_eq!(res.columns, vec!["id", "name", "email"]);
    assert_eq!(res.rows.len(), 3);
    assert_eq!(res.rows[0][1], serde_json::json!("Cecep Saeful Azhar"));
}

#[tokio::test]
async fn mysql_insert_returns_affected_rows() {
    let conn = mysql_conn(false);
    let res = QueryExecutor::execute_query_async(
        &conn,
        "INSERT INTO users (name, email, role) VALUES ('MySQL Live Test', 'mysql-live-test@fathforce.com', 'developer')",
    )
    .await;
    assert!(res.error.is_none(), "mysql insert failed: {:?}", res.error);
    assert_eq!(res.rows_affected, 1);

    let del = QueryExecutor::execute_query_async(
        &conn,
        "DELETE FROM users WHERE email = 'mysql-live-test@fathforce.com'",
    )
    .await;
    assert!(del.error.is_none(), "mysql cleanup failed: {:?}", del.error);
    assert_eq!(del.rows_affected, 1);
}

#[tokio::test]
async fn mysql_read_only_blocks_writes_at_guardrail() {
    let conn = mysql_conn(true);
    let res = QueryExecutor::execute_query_async(
        &conn,
        "INSERT INTO users (name, email, role) VALUES ('Should Fail', 'ro-mysql@fathforce.com', 'dev')",
    )
    .await;
    assert!(res.error.is_some());
    assert!(res.error.unwrap().contains("Query Blocked by Guardrail"));
}

#[tokio::test]
async fn mysql_prod_guardrails_block_unbounded_delete() {
    let mut conn = mysql_conn(false);
    conn.environment = Environment::Production;
    let res = QueryExecutor::execute_query_async(&conn, "DELETE FROM products").await;
    assert!(res.error.is_some());
    assert!(res.error.unwrap().contains("Query Blocked by Guardrail"));
}
