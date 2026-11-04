//! Row edits as data: the UI stages changes, Rust turns them into SQL.
//!
//! The same plan feeds both the preview dialog (`preview_changes`) and the
//! transactional apply (`apply_changes`), so what the user confirms is exactly
//! what runs. Every `UPDATE`/`DELETE` is keyed by the primary key and must
//! affect exactly one row; anything else aborts and rolls the whole set back.

use serde::{Deserialize, Serialize};

use crate::connection::DriverKind;
use crate::error::CatableError;
use crate::sql_builder::{quote_ident, quote_literal, quote_table};

/// Upper bound on statements per change set (a UI mistake must not become a 1M-row write).
pub const MAX_STATEMENTS: usize = 5_000;

/// A cell value as the user typed it. Text is sent as a literal and coerced to the column type by
/// the server (typed editors arrive in a later session).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CellValue {
    Null,
    Text(String),
    /// The column's `DEFAULT` (not available on SQLite).
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPart {
    pub column: String,
    pub value: CellValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnChange {
    pub column: String,
    pub value: CellValue,
    /// The value the user saw. When present it is added to the `WHERE`, so an edit made on top of
    /// someone else's change fails instead of silently overwriting it.
    #[serde(default)]
    pub original: Option<CellValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowUpdate {
    pub key: Vec<KeyPart>,
    pub changes: Vec<ColumnChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowInsert {
    /// Columns left out take their default. An empty list inserts an all-default row.
    pub values: Vec<ColumnValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnValue {
    pub column: String,
    pub value: CellValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowDelete {
    pub key: Vec<KeyPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RowChangeSet {
    pub table: String,
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub updates: Vec<RowUpdate>,
    #[serde(default)]
    pub inserts: Vec<RowInsert>,
    #[serde(default)]
    pub deletes: Vec<RowDelete>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementKind {
    Update,
    Insert,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedStatement {
    pub kind: StatementKind,
    pub sql: String,
    /// Updates and deletes must hit exactly one row.
    pub expect_one_row: bool,
}

/// What `apply_changes` reports after a successful commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyResult {
    pub statements: usize,
    pub rows_affected: u64,
    pub sql: Vec<String>,
}

fn bad(message: impl Into<String>) -> CatableError {
    CatableError::Driver {
        driver: "edit".into(),
        message: message.into(),
    }
}

/// Engines that can edit rows today.
pub fn supports_row_edit(driver: DriverKind) -> bool {
    matches!(
        driver,
        DriverKind::Sqlite | DriverKind::Postgres | DriverKind::Mysql | DriverKind::MariaDb
    )
}

fn value_sql(driver: DriverKind, value: &CellValue) -> Result<String, CatableError> {
    match value {
        CellValue::Null => Ok("NULL".into()),
        CellValue::Text(text) => quote_literal(driver, text),
        CellValue::Default if driver == DriverKind::Sqlite => Err(bad(
            "SQLite has no DEFAULT keyword in UPDATE; type the value instead.",
        )),
        CellValue::Default => Ok("DEFAULT".into()),
    }
}

fn condition_sql(
    driver: DriverKind,
    column: &str,
    value: &CellValue,
) -> Result<String, CatableError> {
    let col = quote_ident(driver, column);
    match value {
        CellValue::Null => Ok(format!("{col} IS NULL")),
        CellValue::Text(text) => Ok(format!("{col} = {}", quote_literal(driver, text)?)),
        CellValue::Default => Err(bad(format!(
            "The key column '{column}' cannot be compared with DEFAULT."
        ))),
    }
}

fn key_where(driver: DriverKind, key: &[KeyPart]) -> Result<Vec<String>, CatableError> {
    if key.is_empty() {
        return Err(bad("A row has no key; it cannot be edited safely."));
    }
    key.iter()
        .map(|part| {
            if part.column.trim().is_empty() {
                Err(bad("A key part has no column name."))
            } else {
                condition_sql(driver, &part.column, &part.value)
            }
        })
        .collect()
}

/// Builds the ordered statements: updates, then inserts, then deletes.
pub fn build_plan(
    driver: DriverKind,
    set: &RowChangeSet,
) -> Result<Vec<PlannedStatement>, CatableError> {
    if !supports_row_edit(driver) {
        return Err(bad("Editing rows is not supported for this database type."));
    }
    if set.table.trim().is_empty() {
        return Err(bad("No table was given."));
    }
    let total = set.updates.len() + set.inserts.len() + set.deletes.len();
    if total == 0 {
        return Err(bad("There are no changes to apply."));
    }
    if total > MAX_STATEMENTS {
        return Err(bad(format!(
            "Too many changes at once ({total}); the limit is {MAX_STATEMENTS}."
        )));
    }

    let table = quote_table(driver, set.schema.as_deref(), &set.table);
    let mut plan = Vec::with_capacity(total);

    for update in &set.updates {
        if update.changes.is_empty() {
            return Err(bad("An update has no changed columns."));
        }
        let assignments = update
            .changes
            .iter()
            .map(|c| {
                if c.column.trim().is_empty() {
                    return Err(bad("A changed cell has no column name."));
                }
                Ok(format!(
                    "{} = {}",
                    quote_ident(driver, &c.column),
                    value_sql(driver, &c.value)?
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut conditions = key_where(driver, &update.key)?;
        for change in &update.changes {
            if let Some(original) = &change.original {
                if *original == CellValue::Default {
                    return Err(bad("An original value cannot be DEFAULT."));
                }
                conditions.push(condition_sql(driver, &change.column, original)?);
            }
        }

        plan.push(PlannedStatement {
            kind: StatementKind::Update,
            sql: format!(
                "UPDATE {table} SET {} WHERE {}",
                assignments.join(", "),
                conditions.join(" AND ")
            ),
            expect_one_row: true,
        });
    }

    for insert in &set.inserts {
        let sql = if insert.values.is_empty() {
            match driver {
                DriverKind::Mysql | DriverKind::MariaDb => {
                    format!("INSERT INTO {table} () VALUES ()")
                }
                _ => format!("INSERT INTO {table} DEFAULT VALUES"),
            }
        } else {
            let mut columns = Vec::with_capacity(insert.values.len());
            let mut values = Vec::with_capacity(insert.values.len());
            for v in &insert.values {
                if v.column.trim().is_empty() {
                    return Err(bad("A new-row cell has no column name."));
                }
                columns.push(quote_ident(driver, &v.column));
                values.push(value_sql(driver, &v.value)?);
            }
            format!(
                "INSERT INTO {table} ({}) VALUES ({})",
                columns.join(", "),
                values.join(", ")
            )
        };
        plan.push(PlannedStatement {
            kind: StatementKind::Insert,
            sql,
            expect_one_row: false,
        });
    }

    for delete in &set.deletes {
        plan.push(PlannedStatement {
            kind: StatementKind::Delete,
            sql: format!(
                "DELETE FROM {table} WHERE {}",
                key_where(driver, &delete.key)?.join(" AND ")
            ),
            expect_one_row: true,
        });
    }

    Ok(plan)
}

/// The SQL a plan will run, for the preview dialog.
pub fn preview(driver: DriverKind, set: &RowChangeSet) -> Result<Vec<String>, CatableError> {
    Ok(build_plan(driver, set)?
        .into_iter()
        .map(|p| p.sql)
        .collect())
}

/// Message for a statement that did not hit exactly one row.
pub fn unexpected_row_count(
    index: usize,
    statement: &PlannedStatement,
    affected: u64,
) -> CatableError {
    let what = match statement.kind {
        StatementKind::Update => "update",
        StatementKind::Delete => "delete",
        StatementKind::Insert => "insert",
    };
    bad(format!(
        "Change {} (an {what}) affected {affected} rows instead of 1. The row was changed or deleted \
         by someone else, or its key is not unique. Nothing was saved.\n{}",
        index + 1,
        statement.sql
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(column: &str, value: &str) -> KeyPart {
        KeyPart {
            column: column.into(),
            value: CellValue::Text(value.into()),
        }
    }

    fn change(column: &str, value: CellValue, original: Option<CellValue>) -> ColumnChange {
        ColumnChange {
            column: column.into(),
            value,
            original,
        }
    }

    fn text(v: &str) -> CellValue {
        CellValue::Text(v.into())
    }

    fn set(table: &str) -> RowChangeSet {
        RowChangeSet {
            table: table.into(),
            ..Default::default()
        }
    }

    #[test]
    fn update_is_keyed_by_primary_key_and_quoted_per_driver() {
        let mut s = set("users");
        s.updates.push(RowUpdate {
            key: vec![key("id", "7")],
            changes: vec![change("name", text("Ada"), None)],
        });
        assert_eq!(
            preview(DriverKind::Postgres, &s).unwrap(),
            vec!["UPDATE \"users\" SET \"name\" = 'Ada' WHERE \"id\" = '7'"]
        );
        assert_eq!(
            preview(DriverKind::Mysql, &s).unwrap(),
            vec!["UPDATE `users` SET `name` = 'Ada' WHERE `id` = '7'"]
        );
    }

    #[test]
    fn null_default_and_composite_keys() {
        let mut s = set("t");
        s.updates.push(RowUpdate {
            key: vec![key("a", "1"), key("b", "2")],
            changes: vec![
                change("x", CellValue::Null, None),
                change("y", CellValue::Default, None),
            ],
        });
        assert_eq!(
            preview(DriverKind::Postgres, &s).unwrap(),
            vec![
                "UPDATE \"t\" SET \"x\" = NULL, \"y\" = DEFAULT WHERE \"a\" = '1' AND \"b\" = '2'"
            ]
        );
        // SQLite has no DEFAULT in UPDATE.
        assert!(preview(DriverKind::Sqlite, &s).is_err());
    }

    #[test]
    fn original_values_guard_against_overwriting_someone_elses_change() {
        let mut s = set("t");
        s.updates.push(RowUpdate {
            key: vec![key("id", "1")],
            changes: vec![
                change("name", text("new"), Some(text("old"))),
                change("note", text("n"), Some(CellValue::Null)),
            ],
        });
        assert_eq!(
            preview(DriverKind::Mysql, &s).unwrap(),
            vec![
                "UPDATE `t` SET `name` = 'new', `note` = 'n' WHERE `id` = '1' AND `name` = 'old' AND `note` IS NULL"
            ]
        );
    }

    #[test]
    fn insert_and_delete_shapes() {
        let mut s = set("t");
        s.schema = Some("public".into());
        s.inserts.push(RowInsert {
            values: vec![
                ColumnValue {
                    column: "id".into(),
                    value: text("9"),
                },
                ColumnValue {
                    column: "name".into(),
                    value: CellValue::Null,
                },
            ],
        });
        s.inserts.push(RowInsert { values: vec![] });
        s.deletes.push(RowDelete {
            key: vec![key("id", "3")],
        });
        assert_eq!(
            preview(DriverKind::Postgres, &s).unwrap(),
            vec![
                "INSERT INTO \"public\".\"t\" (\"id\", \"name\") VALUES ('9', NULL)",
                "INSERT INTO \"public\".\"t\" DEFAULT VALUES",
                "DELETE FROM \"public\".\"t\" WHERE \"id\" = '3'",
            ]
        );
        let mysql = preview(DriverKind::MariaDb, &s).unwrap();
        assert_eq!(mysql[1], "INSERT INTO `public`.`t` () VALUES ()");
    }

    #[test]
    fn statements_are_ordered_updates_then_inserts_then_deletes() {
        let mut s = set("t");
        s.deletes.push(RowDelete {
            key: vec![key("id", "1")],
        });
        s.inserts.push(RowInsert { values: vec![] });
        s.updates.push(RowUpdate {
            key: vec![key("id", "2")],
            changes: vec![change("x", text("1"), None)],
        });
        let kinds: Vec<_> = build_plan(DriverKind::Sqlite, &s)
            .unwrap()
            .iter()
            .map(|p| p.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                StatementKind::Update,
                StatementKind::Insert,
                StatementKind::Delete
            ]
        );
    }

    #[test]
    fn values_cannot_break_out_of_the_literal() {
        let mut s = set("users");
        s.updates.push(RowUpdate {
            key: vec![key("id", "1' OR '1'='1")],
            changes: vec![change("name", text("x'; DROP TABLE users; --"), None)],
        });
        let sql = &preview(DriverKind::Postgres, &s).unwrap()[0];
        assert!(sql.contains("'x''; DROP TABLE users; --'"), "{sql}");
        assert!(sql.contains("\"id\" = '1'' OR ''1''=''1'"), "{sql}");
    }

    #[test]
    fn rejects_unsafe_or_empty_change_sets() {
        assert!(
            preview(DriverKind::Postgres, &set("t")).is_err(),
            "no changes"
        );
        assert!(
            preview(DriverKind::Postgres, &set(" ")).is_err(),
            "no table"
        );

        let mut s = set("t");
        s.updates.push(RowUpdate {
            key: vec![],
            changes: vec![change("a", text("1"), None)],
        });
        assert!(
            preview(DriverKind::Postgres, &s).is_err(),
            "update without a key"
        );

        let mut s = set("t");
        s.deletes.push(RowDelete { key: vec![] });
        assert!(
            preview(DriverKind::Postgres, &s).is_err(),
            "delete without a key"
        );

        let mut s = set("t");
        s.updates.push(RowUpdate {
            key: vec![key("id", "1")],
            changes: vec![],
        });
        assert!(
            preview(DriverKind::Postgres, &s).is_err(),
            "update without changes"
        );

        let mut s = set("t");
        s.deletes.push(RowDelete {
            key: vec![KeyPart {
                column: "id".into(),
                value: CellValue::Default,
            }],
        });
        assert!(
            preview(DriverKind::Postgres, &s).is_err(),
            "DEFAULT as a key"
        );

        assert!(
            preview(DriverKind::DuckDb, &{
                let mut s = set("t");
                s.deletes.push(RowDelete {
                    key: vec![key("id", "1")],
                });
                s
            })
            .is_err()
        );
    }

    #[test]
    fn caps_the_number_of_statements() {
        let mut s = set("t");
        for i in 0..=MAX_STATEMENTS {
            s.deletes.push(RowDelete {
                key: vec![key("id", &i.to_string())],
            });
        }
        assert!(preview(DriverKind::Postgres, &s).is_err());
    }

    #[test]
    fn deserializes_the_ui_payload() {
        let json = r#"{
            "table": "users",
            "updates": [{"key": [{"column": "id", "value": {"kind": "text", "value": "5"}}],
                         "changes": [{"column": "name", "value": {"kind": "null"},
                                      "original": {"kind": "text", "value": "Ada"}}]}],
            "inserts": [{"values": [{"column": "name", "value": {"kind": "default"}}]}]
        }"#;
        let parsed: RowChangeSet = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.updates[0].changes[0].value, CellValue::Null);
        assert_eq!(parsed.updates[0].changes[0].original, Some(text("Ada")));
        assert_eq!(parsed.inserts[0].values[0].value, CellValue::Default);
        assert!(parsed.deletes.is_empty());
    }
}
