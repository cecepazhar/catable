use crate::connection::ConnectionConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryRisk {
    Safe,
    Warning,
    Destructive,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardVerdict {
    pub risk: QueryRisk,
    pub reason: Option<String>,
}

pub struct GuardrailEngine;

impl GuardrailEngine {
    /// Evaluates a SQL string against the safety rules of a connection.
    ///
    /// Rules:
    /// 1. If connection is `read_only`, any non-SELECT/EXPLAIN/PRAGMA/SHOW statement is `Blocked`.
    /// 2. If the statement is a `DROP TABLE`/`TRUNCATE`/`DROP DATABASE`, it is `Blocked` on production, `Destructive` otherwise.
    /// 3. If the statement is a `DELETE` or `UPDATE` with no `WHERE` clause, it is `Blocked` on production, `Destructive` otherwise.
    /// 4. If the statement is an `ALTER TABLE`, `DROP INDEX`, `DROP VIEW`, or targeted `DELETE`/`UPDATE`, it is `Warning`.
    pub fn evaluate(sql: &str, conn: &ConnectionConfig) -> GuardVerdict {
        let trimmed = sql.trim();
        let upper = trimmed.to_uppercase();

        // 1. Read-only gate
        if conn.read_only {
            let is_read = upper.starts_with("SELECT")
                || upper.starts_with("EXPLAIN")
                || upper.starts_with("SHOW")
                || upper.starts_with("DESCRIBE")
                || upper.starts_with("PRAGMA")
                || upper.starts_with("WITH");

            if !is_read {
                return GuardVerdict {
                    risk: QueryRisk::Blocked,
                    reason: Some("Connection is configured as READ-ONLY; write queries are disallowed.".into()),
                };
            }
        }

        // 2. Destructive DDL: DROP / TRUNCATE
        if upper.starts_with("DROP TABLE") || upper.starts_with("DROP DATABASE") || upper.starts_with("TRUNCATE") {
            if conn.is_production() {
                return GuardVerdict {
                    risk: QueryRisk::Blocked,
                    reason: Some("Destructive DDL (DROP/TRUNCATE) is strictly blocked on Production connections.".into()),
                };
            } else {
                return GuardVerdict {
                    risk: QueryRisk::Destructive,
                    reason: Some("Destructive DDL: will permanently delete tables or data.".into()),
                };
            }
        }

        // 3. Unbounded DELETE / UPDATE
        let is_unbounded_delete = upper.starts_with("DELETE FROM") && !upper.contains("WHERE");
        let is_unbounded_update = upper.starts_with("UPDATE") && !upper.contains("WHERE");

        if is_unbounded_delete || is_unbounded_update {
            if conn.is_production() {
                return GuardVerdict {
                    risk: QueryRisk::Blocked,
                    reason: Some("Unbounded DELETE/UPDATE without a WHERE clause is blocked on Production connections.".into()),
                };
            } else {
                return GuardVerdict {
                    risk: QueryRisk::Destructive,
                    reason: Some("Destructive query: Unbounded DELETE/UPDATE affects all rows in the target table.".into()),
                };
            }
        }

        // 4. Schema alterations & targeted mutations: Warning
        if upper.starts_with("ALTER TABLE") || upper.starts_with("DROP INDEX") || upper.starts_with("DROP VIEW") {
            return GuardVerdict {
                risk: QueryRisk::Warning,
                reason: Some("Schema modification: verify target object definition before applying.".into()),
            };
        }

        if upper.starts_with("DELETE FROM") || upper.starts_with("UPDATE") {
            return GuardVerdict {
                risk: QueryRisk::Warning,
                reason: Some("Data modification: verify WHERE clause conditions before executing.".into()),
            };
        }

        GuardVerdict {
            risk: QueryRisk::Safe,
            reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{DriverKind, Environment};

    fn make_conn(env: Environment, read_only: bool) -> ConnectionConfig {
        ConnectionConfig {
            id: "test".into(),
            name: "test".into(),
            driver: DriverKind::Sqlite,
            environment: env,
            host: "".into(),
            port: 0,
            database: "test.db".into(),
            user: "".into(),
            password: None,
            read_only,
            ssh_tunnel: None,
        }
    }

    #[test]
    fn blocks_write_in_read_only() {
        let conn = make_conn(Environment::Development, true);
        let v = GuardrailEngine::evaluate("DELETE FROM users WHERE id = 1", &conn);
        assert_eq!(v.risk, QueryRisk::Blocked);

        let v2 = GuardrailEngine::evaluate("SELECT * FROM users", &conn);
        assert_eq!(v2.risk, QueryRisk::Safe);
    }

    #[test]
    fn blocks_unbounded_delete_on_production() {
        let conn = make_conn(Environment::Production, false);
        let v = GuardrailEngine::evaluate("DELETE FROM users", &conn);
        assert_eq!(v.risk, QueryRisk::Blocked);

        let v_bounded = GuardrailEngine::evaluate("DELETE FROM users WHERE id = 42", &conn);
        assert_eq!(v_bounded.risk, QueryRisk::Warning);

        let dev_conn = make_conn(Environment::Development, false);
        let v_drop = GuardrailEngine::evaluate("DROP TABLE users", &dev_conn);
        assert_eq!(v_drop.risk, QueryRisk::Destructive);

        let v_select = GuardrailEngine::evaluate("SELECT * FROM users", &dev_conn);
        assert_eq!(v_select.risk, QueryRisk::Safe);
    }
}
