use crate::connection::{ConnectionConfig, DriverKind};
use serde::{Deserialize, Serialize};
use sqlparser::ast::{
    AlterTableOperation, Expr, ObjectType, Query, SetExpr, Statement, Value, visit_statements,
};
use sqlparser::dialect::{
    Dialect, DuckDbDialect, GenericDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect,
};
use sqlparser::parser::Parser;
use std::ops::ControlFlow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryRisk {
    Safe,
    Warning,
    Destructive,
    Blocked,
}

impl QueryRisk {
    fn rank(self) -> u8 {
        match self {
            QueryRisk::Safe => 0,
            QueryRisk::Warning => 1,
            QueryRisk::Destructive => 2,
            QueryRisk::Blocked => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardVerdict {
    pub risk: QueryRisk,
    pub reason: Option<String>,
}

impl GuardVerdict {
    fn safe() -> Self {
        Self {
            risk: QueryRisk::Safe,
            reason: None,
        }
    }

    fn new(risk: QueryRisk, reason: impl Into<String>) -> Self {
        Self {
            risk,
            reason: Some(reason.into()),
        }
    }

    /// Keeps the more severe of two verdicts (the first wins a tie).
    fn worst(self, other: GuardVerdict) -> GuardVerdict {
        if other.risk.rank() > self.risk.rank() {
            other
        } else {
            self
        }
    }
}

pub struct GuardrailEngine;

fn dialect_for(driver: DriverKind) -> Box<dyn Dialect> {
    match driver {
        DriverKind::Postgres => Box::new(PostgreSqlDialect {}),
        DriverKind::Mysql | DriverKind::MariaDb => Box::new(MySqlDialect {}),
        DriverKind::Sqlite => Box::new(SQLiteDialect {}),
        DriverKind::DuckDb => Box::new(DuckDbDialect {}),
        DriverKind::Redis => Box::new(GenericDialect {}),
    }
}

/// A `WHERE` that does not restrict anything (`1 = 1`, `true`, `1`, `x OR 1 = 1`…).
fn always_true(expr: &Expr) -> bool {
    use sqlparser::ast::BinaryOperator as Op;
    match expr {
        Expr::Nested(inner) => always_true(inner),
        Expr::Value(v) => match &v.value {
            Value::Boolean(b) => *b,
            Value::Number(n, _) => n.parse::<f64>().map(|n| n != 0.0).unwrap_or(false),
            _ => false,
        },
        Expr::BinaryOp { left, op, right } => match op {
            Op::Or => always_true(left) || always_true(right),
            Op::And => always_true(left) && always_true(right),
            Op::Eq | Op::GtEq | Op::LtEq => match (left.as_ref(), right.as_ref()) {
                (Expr::Value(a), Expr::Value(b)) => a.value == b.value,
                _ => false,
            },
            _ => false,
        },
        _ => false,
    }
}

fn unbounded(selection: &Option<Expr>) -> bool {
    selection.as_ref().is_none_or(always_true)
}

fn query_writes(query: &Query) -> bool {
    fn body_writes(body: &SetExpr) -> bool {
        match body {
            SetExpr::Select(select) => select.into.is_some(),
            SetExpr::Query(q) => query_writes(q),
            SetExpr::SetOperation { left, right, .. } => body_writes(left) || body_writes(right),
            SetExpr::Insert(_) | SetExpr::Update(_) | SetExpr::Delete(_) | SetExpr::Merge(_) => {
                true
            }
            _ => false,
        }
    }
    let in_cte = query
        .with
        .as_ref()
        .is_some_and(|w| w.cte_tables.iter().any(|cte| query_writes(&cte.query)));
    in_cte || body_writes(&query.body)
}

/// SQLite pragmas that only report information when called with an argument.
const READ_PRAGMAS: &[&str] = &[
    "table_info",
    "table_xinfo",
    "table_list",
    "index_list",
    "index_info",
    "index_xinfo",
    "foreign_key_list",
    "foreign_key_check",
    "database_list",
    "collation_list",
    "function_list",
    "module_list",
    "pragma_list",
    "integrity_check",
    "quick_check",
    "compile_options",
];

/// `PRAGMA table_info(users)` and friends: the parser rejects an identifier argument, so these
/// arrive as unparsed text. Only the introspection pragmas, and never the `=` form.
fn is_read_pragma_call(upper_sql: &str) -> bool {
    let Some(rest) = upper_sql.strip_prefix("PRAGMA ") else {
        return false;
    };
    if rest.contains('=') {
        return false;
    }
    let name = rest
        .trim_start()
        .split(|c: char| c == '(' || c.is_whitespace() || c == ';')
        .next()
        .unwrap_or("")
        .to_lowercase();
    let short = name.rsplit('.').next().unwrap_or(&name);
    READ_PRAGMAS.contains(&short)
}

/// Statements that only read. Anything not listed here counts as a write for read-only
/// connections (a new statement kind must be allowed on purpose, never by accident).
fn is_read(stmt: &Statement) -> bool {
    match stmt {
        Statement::Query(q) => !query_writes(q),
        Statement::Explain {
            analyze, statement, ..
        } => !*analyze || is_read(statement),
        Statement::ExplainTable { .. }
        | Statement::ShowFunctions { .. }
        | Statement::ShowVariable { .. }
        | Statement::ShowStatus { .. }
        | Statement::ShowVariables { .. }
        | Statement::ShowCreate { .. }
        | Statement::ShowColumns { .. }
        | Statement::ShowDatabases { .. }
        | Statement::ShowSchemas { .. }
        | Statement::ShowTables { .. }
        | Statement::ShowViews { .. }
        | Statement::ShowCollation { .. } => true,
        // `PRAGMA name` reads and `PRAGMA name = value` writes. `PRAGMA name(arg)` is a query for
        // the introspection pragmas below and a setting for everything else (`journal_mode(WAL)`).
        Statement::Pragma { name, value, is_eq } => {
            if *is_eq {
                false
            } else if value.is_none() {
                true
            } else {
                let full = name.to_string().to_lowercase();
                let short = full.rsplit('.').next().unwrap_or(&full);
                READ_PRAGMAS.contains(&short)
            }
        }
        _ => false,
    }
}

fn classify(stmt: &Statement, conn: &ConnectionConfig) -> GuardVerdict {
    let production = conn.is_production();
    let destructive = |what: &str| {
        if production {
            GuardVerdict::new(
                QueryRisk::Blocked,
                format!("Destructive DDL ({what}) is strictly blocked on Production connections."),
            )
        } else {
            GuardVerdict::new(
                QueryRisk::Destructive,
                format!("Destructive DDL: {what} will permanently delete tables, columns or data."),
            )
        }
    };
    let bulk = |what: &str| {
        if production {
            GuardVerdict::new(
                QueryRisk::Blocked,
                format!(
                    "Unbounded {what} without a WHERE clause is blocked on Production connections."
                ),
            )
        } else {
            GuardVerdict::new(
                QueryRisk::Destructive,
                format!(
                    "Destructive query: unbounded {what} affects all rows in the target table."
                ),
            )
        }
    };

    match stmt {
        Statement::Truncate(_) => destructive("TRUNCATE"),
        Statement::Drop { object_type, .. } => match object_type {
            ObjectType::Table => destructive("DROP TABLE"),
            ObjectType::Database => destructive("DROP DATABASE"),
            ObjectType::Schema => destructive("DROP SCHEMA"),
            _ => GuardVerdict::new(
                QueryRisk::Warning,
                "Schema modification: verify target object definition before applying.",
            ),
        },
        Statement::Delete(delete) => {
            if unbounded(&delete.selection) {
                bulk("DELETE")
            } else {
                GuardVerdict::new(
                    QueryRisk::Warning,
                    "Data modification: verify WHERE clause conditions before executing.",
                )
            }
        }
        Statement::Update(update) => {
            if unbounded(&update.selection) {
                bulk("UPDATE")
            } else {
                GuardVerdict::new(
                    QueryRisk::Warning,
                    "Data modification: verify WHERE clause conditions before executing.",
                )
            }
        }
        Statement::AlterTable(alter) => {
            let drops = alter.operations.iter().any(|op| {
                matches!(
                    op,
                    AlterTableOperation::DropColumn { .. }
                        | AlterTableOperation::DropPrimaryKey { .. }
                        | AlterTableOperation::DropConstraint { .. }
                )
            });
            if drops {
                destructive("ALTER TABLE … DROP")
            } else {
                GuardVerdict::new(
                    QueryRisk::Warning,
                    "Schema modification: verify target object definition before applying.",
                )
            }
        }
        Statement::Insert(insert) if insert.replace_into => GuardVerdict::new(
            QueryRisk::Warning,
            "REPLACE INTO deletes any existing row with the same key before inserting.",
        ),
        Statement::Merge(_) => GuardVerdict::new(
            QueryRisk::Warning,
            "MERGE can insert, update and delete rows in one statement.",
        ),
        Statement::Grant(_) | Statement::Revoke(_) => {
            GuardVerdict::new(QueryRisk::Warning, "Changes who may access this database.")
        }
        // `COPY … FROM` loads rows from a file or stdin; `COPY … TO` only reads.
        Statement::Copy { to: false, .. } => GuardVerdict::new(
            QueryRisk::Warning,
            "COPY FROM writes rows from a file into the table.",
        ),
        Statement::LoadData { .. } => GuardVerdict::new(
            QueryRisk::Warning,
            "LOAD DATA writes rows from a file into the table.",
        ),
        Statement::Query(q) if query_writes(q) => GuardVerdict::new(
            QueryRisk::Warning,
            "This query writes data (SELECT … INTO or a data-modifying WITH clause).",
        ),
        _ => GuardVerdict::safe(),
    }
}

impl GuardrailEngine {
    /// Evaluates a SQL string against the safety rules of a connection.
    ///
    /// The SQL is parsed (per dialect) and EVERY statement in it is judged, including statements
    /// nested in a `WITH` clause; the most severe verdict wins. Comments, string literals and
    /// multiple statements therefore cannot hide a write:
    ///
    /// 1. A `read_only` connection allows only read statements (see [`is_read`]).
    /// 2. `DROP TABLE/DATABASE/SCHEMA`, `TRUNCATE` and `ALTER TABLE … DROP …` are `Blocked` on
    ///    production and `Destructive` elsewhere.
    /// 3. `DELETE`/`UPDATE` without a restricting `WHERE` (including `WHERE 1=1`) is `Blocked` on
    ///    production and `Destructive` elsewhere; with a real `WHERE` it is a `Warning`.
    /// 4. Other schema changes, `REPLACE`, `MERGE`, grants and bulk loads are a `Warning`.
    ///
    /// If the SQL cannot be parsed the older text rules are applied, and on production an
    /// unrecognised statement is at least a `Warning`.
    pub fn evaluate(sql: &str, conn: &ConnectionConfig) -> GuardVerdict {
        let dialect = dialect_for(conn.driver);
        let statements = match Parser::parse_sql(dialect.as_ref(), sql) {
            Ok(statements) => statements,
            Err(_) => return Self::evaluate_unparsed(sql, conn),
        };

        let mut verdict = GuardVerdict::safe();
        for statement in &statements {
            verdict = verdict.worst(Self::evaluate_statement(statement, conn));
        }
        verdict
    }

    fn evaluate_statement(root: &Statement, conn: &ConnectionConfig) -> GuardVerdict {
        // EXPLAIN without ANALYZE only plans; with ANALYZE it runs the inner statement.
        if let Statement::Explain {
            analyze, statement, ..
        } = root
        {
            return if *analyze {
                Self::evaluate_statement(statement, conn)
            } else {
                GuardVerdict::safe()
            };
        }

        // The statement itself plus anything nested in it (CTEs, sub-statements). Each visited
        // node is judged on the spot: the visitor hands out short-lived references.
        let mut verdict = GuardVerdict::safe();
        let mut writes = false;
        let mut visited = 0usize;
        let _ = visit_statements(root, |node| {
            visited += 1;
            writes |= !is_read(node);
            verdict =
                std::mem::replace(&mut verdict, GuardVerdict::safe()).worst(classify(node, conn));
            ControlFlow::<()>::Continue(())
        });
        if visited == 0 {
            writes = !is_read(root);
            verdict = classify(root, conn);
        }

        if conn.read_only && writes {
            return GuardVerdict::new(
                QueryRisk::Blocked,
                "Connection is configured as READ-ONLY; write queries are disallowed.",
            );
        }
        verdict
    }

    /// Text-based rules for SQL the parser does not understand.
    fn evaluate_unparsed(sql: &str, conn: &ConnectionConfig) -> GuardVerdict {
        let upper = sql.trim().to_uppercase();

        if conn.read_only {
            // Unparsed SQL cannot be proven harmless, so only a single plain SHOW/DESCRIBE/EXPLAIN
            // (statements the parser merely does not know) is let through.
            let single = !upper.trim_end_matches(';').contains(';');
            let metadata = ["SHOW ", "DESCRIBE ", "DESC ", "EXPLAIN "]
                .iter()
                .any(|p| upper.starts_with(p))
                || is_read_pragma_call(&upper);
            if !(single && metadata) {
                return GuardVerdict::new(
                    QueryRisk::Blocked,
                    "Connection is READ-ONLY and this SQL could not be analysed, so it is not allowed.",
                );
            }
        }

        if conn.is_production() && !upper.is_empty() {
            return GuardVerdict::new(
                QueryRisk::Warning,
                "This SQL could not be analysed; review it carefully before running it on Production.",
            );
        }
        GuardVerdict::safe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::Environment;

    fn conn(driver: DriverKind, env: Environment, read_only: bool) -> ConnectionConfig {
        let mut c = ConnectionConfig::sandbox();
        c.driver = driver;
        c.environment = env;
        c.read_only = read_only;
        c
    }

    fn risk(driver: DriverKind, env: Environment, read_only: bool, sql: &str) -> QueryRisk {
        GuardrailEngine::evaluate(sql, &conn(driver, env, read_only)).risk
    }

    const DEV: Environment = Environment::Development;
    const PROD: Environment = Environment::Production;
    use DriverKind::{MariaDb, Mysql, Postgres, Sqlite};

    #[test]
    fn blocks_write_in_read_only() {
        let c = conn(Sqlite, DEV, true);
        assert_eq!(
            GuardrailEngine::evaluate("DELETE FROM users WHERE id = 1", &c).risk,
            QueryRisk::Blocked
        );
        assert_eq!(
            GuardrailEngine::evaluate("SELECT * FROM users", &c).risk,
            QueryRisk::Safe
        );
    }

    #[test]
    fn blocks_unbounded_delete_on_production() {
        let c = conn(Sqlite, PROD, false);
        assert_eq!(
            GuardrailEngine::evaluate("DELETE FROM users", &c).risk,
            QueryRisk::Blocked
        );
        assert_eq!(
            GuardrailEngine::evaluate("DELETE FROM users WHERE id = 42", &c).risk,
            QueryRisk::Warning
        );

        let dev = conn(Sqlite, DEV, false);
        assert_eq!(
            GuardrailEngine::evaluate("DROP TABLE users", &dev).risk,
            QueryRisk::Destructive
        );
        assert_eq!(
            GuardrailEngine::evaluate("SELECT * FROM users", &dev).risk,
            QueryRisk::Safe
        );
    }

    /// The bypasses listed in the audit: every one of these used to slip through the text rules.
    #[test]
    fn comments_in_front_do_not_hide_a_delete() {
        for sql in [
            "/* x */ DELETE FROM t",
            "-- c\nDELETE FROM t",
            "/* a */ /* b */ UPDATE t SET a = 1",
        ] {
            assert_eq!(
                risk(Postgres, PROD, false, sql),
                QueryRisk::Blocked,
                "{sql}"
            );
            assert_eq!(
                risk(Postgres, DEV, false, sql),
                QueryRisk::Destructive,
                "{sql}"
            );
        }
    }

    #[test]
    fn a_second_statement_cannot_hide_behind_a_select() {
        let sql = "SELECT 1; DELETE FROM t";
        assert_eq!(
            risk(Postgres, DEV, true, sql),
            QueryRisk::Blocked,
            "read-only"
        );
        assert_eq!(
            risk(Postgres, PROD, false, sql),
            QueryRisk::Blocked,
            "production"
        );
        assert_eq!(
            risk(Mysql, DEV, false, "SELECT 1; DROP TABLE t"),
            QueryRisk::Destructive
        );
    }

    #[test]
    fn a_writing_cte_is_not_a_read() {
        let sql = "WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x";
        assert_eq!(risk(Postgres, DEV, true, sql), QueryRisk::Blocked);
        assert_eq!(
            risk(Postgres, DEV, false, sql),
            QueryRisk::Destructive,
            "the DELETE inside is unbounded"
        );
        // A plain read-only CTE is fine.
        assert_eq!(
            risk(Postgres, DEV, true, "WITH x AS (SELECT 1) SELECT * FROM x"),
            QueryRisk::Safe
        );
    }

    #[test]
    fn the_word_where_inside_a_string_is_not_a_where_clause() {
        assert_eq!(
            risk(Postgres, PROD, false, "UPDATE t SET note = 'WHERE'"),
            QueryRisk::Blocked
        );
        assert_eq!(
            risk(Mysql, PROD, false, "UPDATE t SET note = 'x WHERE y'"),
            QueryRisk::Blocked
        );
    }

    #[test]
    fn a_where_that_matches_everything_is_unbounded() {
        for sql in [
            "DELETE FROM t WHERE 1=1",
            "DELETE FROM t WHERE true",
            "DELETE FROM t WHERE 1",
            "DELETE FROM t WHERE ('a' = 'a')",
            "DELETE FROM t WHERE id = 5 OR 1 = 1",
        ] {
            assert_eq!(
                risk(Postgres, PROD, false, sql),
                QueryRisk::Blocked,
                "{sql}"
            );
        }
        for sql in [
            "DELETE FROM t WHERE id = 5",
            "DELETE FROM t WHERE id = 5 AND 1 = 1",
            "DELETE FROM t WHERE 1 = 2",
        ] {
            assert_eq!(
                risk(Postgres, PROD, false, sql),
                QueryRisk::Warning,
                "{sql}"
            );
        }
    }

    #[test]
    fn destructive_ddl_is_blocked_on_production_and_flagged_elsewhere() {
        for sql in [
            "TRUNCATE TABLE t",
            "DROP TABLE t",
            "DROP SCHEMA s",
            "DROP DATABASE d",
            "ALTER TABLE t DROP COLUMN c",
        ] {
            assert_eq!(
                risk(Postgres, PROD, false, sql),
                QueryRisk::Blocked,
                "{sql}"
            );
            assert_eq!(
                risk(Postgres, DEV, false, sql),
                QueryRisk::Destructive,
                "{sql}"
            );
        }
        for sql in [
            "ALTER TABLE t ADD COLUMN c INT",
            "DROP INDEX i",
            "DROP VIEW v",
        ] {
            assert_eq!(risk(Postgres, DEV, false, sql), QueryRisk::Warning, "{sql}");
        }
    }

    #[test]
    fn select_into_and_other_writes_are_not_reads() {
        assert_eq!(
            risk(Postgres, DEV, true, "SELECT * INTO new_t FROM t"),
            QueryRisk::Blocked
        );
        assert_eq!(
            risk(Postgres, DEV, false, "SELECT * INTO new_t FROM t"),
            QueryRisk::Warning
        );
        for sql in [
            "INSERT INTO t VALUES (1)",
            "CREATE TABLE x (a INT)",
            "GRANT SELECT ON t TO u",
            "REVOKE SELECT ON t FROM u",
            "SET search_path = x",
        ] {
            assert_eq!(risk(Postgres, DEV, true, sql), QueryRisk::Blocked, "{sql}");
        }
        assert_eq!(
            risk(MariaDb, DEV, true, "REPLACE INTO t VALUES (1)"),
            QueryRisk::Blocked
        );
        assert_eq!(
            risk(MariaDb, DEV, false, "REPLACE INTO t VALUES (1)"),
            QueryRisk::Warning
        );
        assert_eq!(
            risk(Postgres, DEV, false, "GRANT SELECT ON t TO u"),
            QueryRisk::Warning
        );
    }

    #[test]
    fn bulk_file_loads_are_never_read_only_safe() {
        // MySQL's LOAD DATA INFILE and Hive-style LOAD DATA INPATH both write rows from a file.
        for (driver, sql) in [
            (Mysql, "LOAD DATA INFILE '/tmp/x.csv' INTO TABLE t"),
            (
                MariaDb,
                "LOAD DATA LOCAL INFILE '/tmp/x.csv' INTO TABLE t FIELDS TERMINATED BY ','",
            ),
            (Postgres, "COPY t FROM '/tmp/x.csv'"),
        ] {
            assert_eq!(
                risk(driver, DEV, true, sql),
                QueryRisk::Blocked,
                "read-only: {sql}"
            );
            assert_ne!(
                risk(driver, PROD, false, sql),
                QueryRisk::Safe,
                "production: {sql}"
            );
        }
    }

    #[test]
    fn explain_only_plans_unless_analyze_runs_it() {
        assert_eq!(
            risk(Postgres, DEV, true, "EXPLAIN DELETE FROM t"),
            QueryRisk::Safe
        );
        assert_eq!(
            risk(Postgres, DEV, true, "EXPLAIN ANALYZE DELETE FROM t"),
            QueryRisk::Blocked
        );
        assert_eq!(
            risk(Postgres, DEV, true, "EXPLAIN SELECT * FROM t"),
            QueryRisk::Safe
        );
    }

    #[test]
    fn read_statements_are_allowed_on_read_only_connections() {
        for (driver, sql) in [
            (Mysql, "SHOW TABLES"),
            (Mysql, "DESCRIBE users"),
            (Postgres, "SHOW search_path"),
            (Sqlite, "PRAGMA table_info(users)"),
            (Sqlite, "SELECT 1"),
        ] {
            assert_eq!(risk(driver, DEV, true, sql), QueryRisk::Safe, "{sql}");
        }
        assert_eq!(
            risk(Sqlite, DEV, true, "PRAGMA journal_mode = DELETE"),
            QueryRisk::Blocked
        );
    }

    #[test]
    fn the_most_severe_statement_wins() {
        assert_eq!(
            risk(
                Postgres,
                DEV,
                false,
                "SELECT 1; UPDATE t SET a = 1 WHERE id = 1; TRUNCATE t"
            ),
            QueryRisk::Destructive
        );
        assert_eq!(
            risk(Postgres, DEV, false, "SELECT 1; SELECT 2"),
            QueryRisk::Safe
        );
        assert_eq!(risk(Postgres, DEV, false, "   "), QueryRisk::Safe);
        assert_eq!(
            risk(Postgres, DEV, false, "-- only a comment"),
            QueryRisk::Safe
        );
    }

    #[test]
    fn unparseable_sql_is_never_treated_as_obviously_safe_on_production() {
        let sql = "THIS IS NOT SQL AT ALL";
        assert_eq!(risk(Postgres, PROD, false, sql), QueryRisk::Warning);
        assert_eq!(risk(Postgres, DEV, false, sql), QueryRisk::Safe);
        assert_eq!(risk(Postgres, DEV, true, sql), QueryRisk::Blocked);
        // …and cannot smuggle a second statement past a read-only connection.
        assert_eq!(
            risk(Postgres, DEV, true, "SHOW FOO BAR BAZ; DROP TABLE t"),
            QueryRisk::Blocked
        );
    }
}
