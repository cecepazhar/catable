//! Table structure: columns with their defaults, keys, foreign keys, indexes and the `CREATE TABLE`
//! text. Read from each engine's own catalog, so the UI shows what the database actually has.

use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::connection::DriverKind;
use crate::error::CatableError;
use crate::sql_builder::{quote_ident, quote_table};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnDetail {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// The default expression exactly as the engine reports it.
    pub default: Option<String>,
    pub is_pk: bool,
    pub is_auto_increment: bool,
    pub comment: Option<String>,
    pub ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignKey {
    pub name: Option<String>,
    pub columns: Vec<String>,
    pub ref_table: String,
    /// Empty when the engine reports the referenced primary key implicitly (SQLite).
    pub ref_columns: Vec<String>,
    pub on_update: Option<String>,
    pub on_delete: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub primary: bool,
    pub method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableStructure {
    pub table: String,
    pub schema: Option<String>,
    pub columns: Vec<ColumnDetail>,
    pub foreign_keys: Vec<ForeignKey>,
    pub indexes: Vec<IndexInfo>,
    /// `CREATE TABLE …` (plus index statements); `None` when the engine cannot produce it.
    pub ddl: Option<String>,
}

fn fail(driver: &str, e: impl Display) -> CatableError {
    CatableError::Driver {
        driver: driver.into(),
        message: e.to_string(),
    }
}

fn mark_primary_keys(columns: &mut [ColumnDetail], indexes: &[IndexInfo]) {
    for index in indexes.iter().filter(|i| i.primary) {
        for column in columns.iter_mut() {
            if index.columns.contains(&column.name) {
                column.is_pk = true;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// PostgreSQL
// ---------------------------------------------------------------------------------------------

const PG_SCHEMA: &str = "COALESCE($2::text, current_schema())";

pub async fn structure_postgres(
    client: &tokio_postgres::Client,
    table: &str,
    schema: Option<&str>,
) -> Result<TableStructure, CatableError> {
    let err = |e: tokio_postgres::Error| fail("postgres", e);

    let column_rows = client
        .query(
            format!(
                "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, \
                        pg_get_expr(d.adbin, d.adrelid), \
                        CASE a.attidentity WHEN 'a' THEN 'ALWAYS' WHEN 'd' THEN 'BY DEFAULT' END, \
                        col_description(a.attrelid, a.attnum), a.attnum::int \
                 FROM pg_attribute a \
                 JOIN pg_class c ON c.oid = a.attrelid \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                 WHERE c.relname = $1 AND n.nspname = {PG_SCHEMA} \
                       AND a.attnum > 0 AND NOT a.attisdropped \
                 ORDER BY a.attnum"
            )
            .as_str(),
            &[&table, &schema],
        )
        .await
        .map_err(err)?;

    if column_rows.is_empty() {
        return Err(fail("postgres", format!("Table '{table}' was not found.")));
    }

    // (column details, identity kind) kept together so the DDL can print GENERATED … AS IDENTITY.
    let mut identity: Vec<Option<String>> = Vec::new();
    let mut columns: Vec<ColumnDetail> = Vec::new();
    for r in &column_rows {
        let default: Option<String> = r.get(3);
        let ident: Option<String> = r.get(4);
        let auto = ident.is_some()
            || default
                .as_deref()
                .is_some_and(|d| d.starts_with("nextval("));
        identity.push(ident);
        columns.push(ColumnDetail {
            name: r.get(0),
            data_type: r.get(1),
            nullable: r.get(2),
            default,
            is_pk: false,
            is_auto_increment: auto,
            comment: r.get(5),
            ordinal: r.get::<_, i32>(6).max(0) as u32,
        });
    }

    let index_rows = client
        .query(
            format!(
                "SELECT i.relname::text, ix.indisunique, ix.indisprimary, am.amname::text, \
                        array_agg(a.attname::text ORDER BY k.ord) \
                 FROM pg_index ix \
                 JOIN pg_class t ON t.oid = ix.indrelid \
                 JOIN pg_class i ON i.oid = ix.indexrelid \
                 JOIN pg_namespace n ON n.oid = t.relnamespace \
                 JOIN pg_am am ON am.oid = i.relam \
                 CROSS JOIN LATERAL unnest(ix.indkey) WITH ORDINALITY AS k(attnum, ord) \
                 JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = k.attnum \
                 WHERE t.relname = $1 AND n.nspname = {PG_SCHEMA} \
                 GROUP BY i.relname, ix.indisunique, ix.indisprimary, am.amname \
                 ORDER BY ix.indisprimary DESC, i.relname"
            )
            .as_str(),
            &[&table, &schema],
        )
        .await
        .map_err(err)?;
    let indexes: Vec<IndexInfo> = index_rows
        .iter()
        .map(|r| IndexInfo {
            name: r.get(0),
            unique: r.get(1),
            primary: r.get(2),
            method: Some(r.get(3)),
            columns: r.get(4),
        })
        .collect();
    mark_primary_keys(&mut columns, &indexes);

    let rule = |col: &str| {
        format!(
            "CASE {col} WHEN 'a' THEN 'NO ACTION' WHEN 'r' THEN 'RESTRICT' WHEN 'c' THEN 'CASCADE' \
             WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT' END"
        )
    };
    let fk_rows = client
        .query(
            format!(
                "SELECT con.conname::text, \
                   (SELECT array_agg(att.attname::text ORDER BY u.ord) \
                      FROM unnest(con.conkey) WITH ORDINALITY AS u(attnum, ord) \
                      JOIN pg_attribute att ON att.attrelid = con.conrelid AND att.attnum = u.attnum), \
                   fc.relname::text, \
                   (SELECT array_agg(att.attname::text ORDER BY u.ord) \
                      FROM unnest(con.confkey) WITH ORDINALITY AS u(attnum, ord) \
                      JOIN pg_attribute att ON att.attrelid = con.confrelid AND att.attnum = u.attnum), \
                   {}, {} \
                 FROM pg_constraint con \
                 JOIN pg_class t ON t.oid = con.conrelid \
                 JOIN pg_namespace n ON n.oid = t.relnamespace \
                 JOIN pg_class fc ON fc.oid = con.confrelid \
                 WHERE con.contype = 'f' AND t.relname = $1 AND n.nspname = {PG_SCHEMA} \
                 ORDER BY con.conname",
                rule("con.confupdtype"),
                rule("con.confdeltype")
            )
            .as_str(),
            &[&table, &schema],
        )
        .await
        .map_err(err)?;
    let foreign_keys: Vec<ForeignKey> = fk_rows
        .iter()
        .map(|r| ForeignKey {
            name: Some(r.get(0)),
            columns: r.get(1),
            ref_table: r.get(2),
            ref_columns: r.get(3),
            on_update: r.get(4),
            on_delete: r.get(5),
        })
        .collect();

    // DDL: columns, then every constraint as the server itself prints it, then plain indexes.
    let constraint_rows = client
        .query(
            format!(
                "SELECT con.conname::text, pg_get_constraintdef(con.oid) \
                 FROM pg_constraint con \
                 JOIN pg_class t ON t.oid = con.conrelid \
                 JOIN pg_namespace n ON n.oid = t.relnamespace \
                 WHERE t.relname = $1 AND n.nspname = {PG_SCHEMA} AND con.contype IN ('p','u','f','c') \
                 ORDER BY CASE con.contype WHEN 'p' THEN 0 WHEN 'u' THEN 1 WHEN 'c' THEN 2 ELSE 3 END, con.conname"
            )
            .as_str(),
            &[&table, &schema],
        )
        .await
        .map_err(err)?;
    let loose_index_rows = client
        .query(
            format!(
                "SELECT pg_get_indexdef(ix.indexrelid) \
                 FROM pg_index ix \
                 JOIN pg_class t ON t.oid = ix.indrelid \
                 JOIN pg_namespace n ON n.oid = t.relnamespace \
                 WHERE t.relname = $1 AND n.nspname = {PG_SCHEMA} \
                       AND NOT EXISTS (SELECT 1 FROM pg_constraint c WHERE c.conindid = ix.indexrelid) \
                 ORDER BY 1"
            )
            .as_str(),
            &[&table, &schema],
        )
        .await
        .map_err(err)?;

    let driver = DriverKind::Postgres;
    let mut lines: Vec<String> = columns
        .iter()
        .zip(&identity)
        .map(|(c, ident)| {
            let mut line = format!("  {} {}", quote_ident(driver, &c.name), c.data_type);
            if let Some(kind) = ident {
                line.push_str(&format!(" GENERATED {kind} AS IDENTITY"));
            } else if let Some(default) = &c.default {
                line.push_str(&format!(" DEFAULT {default}"));
            }
            if !c.nullable {
                line.push_str(" NOT NULL");
            }
            line
        })
        .collect();
    for r in &constraint_rows {
        let name: String = r.get(0);
        let definition: String = r.get(1);
        lines.push(format!(
            "  CONSTRAINT {} {definition}",
            quote_ident(driver, &name)
        ));
    }
    let mut ddl = format!(
        "CREATE TABLE {} (\n{}\n);",
        quote_table(driver, schema, table),
        lines.join(",\n")
    );
    for r in &loose_index_rows {
        let definition: String = r.get(0);
        ddl.push_str(&format!("\n{definition};"));
    }

    Ok(TableStructure {
        table: table.into(),
        schema: schema.map(str::to_string),
        columns,
        foreign_keys,
        indexes,
        ddl: Some(ddl),
    })
}

// ---------------------------------------------------------------------------------------------
// MySQL / MariaDB
// ---------------------------------------------------------------------------------------------

pub async fn structure_mysql(
    conn: &mut mysql_async::Conn,
    driver: DriverKind,
    table: &str,
    schema: Option<&str>,
) -> Result<TableStructure, CatableError> {
    use mysql_async::prelude::Queryable;
    let err = |e: mysql_async::Error| fail("mysql", e);
    let schema_owned = schema.map(str::to_string);

    let column_rows: Vec<(String, String, String, Option<String>, String, String, i64)> = conn
        .exec(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, EXTRA, COLUMN_COMMENT, \
                    CAST(ORDINAL_POSITION AS SIGNED) \
             FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = COALESCE(?, DATABASE()) AND TABLE_NAME = ? \
             ORDER BY ORDINAL_POSITION",
            (schema_owned.clone(), table.to_string()),
        )
        .await
        .map_err(err)?;
    if column_rows.is_empty() {
        return Err(fail("mysql", format!("Table '{table}' was not found.")));
    }
    let mut columns: Vec<ColumnDetail> = column_rows
        .into_iter()
        .map(
            |(name, data_type, nullable, default, extra, comment, ordinal)| ColumnDetail {
                name,
                data_type,
                nullable: nullable.eq_ignore_ascii_case("YES"),
                default,
                is_pk: false,
                is_auto_increment: extra.to_lowercase().contains("auto_increment"),
                comment: Some(comment).filter(|c| !c.is_empty()),
                ordinal: ordinal.max(0) as u32,
            },
        )
        .collect();

    let stat_rows: Vec<(String, String, i64, i64, String)> = conn
        .exec(
            "SELECT INDEX_NAME, COLUMN_NAME, CAST(NON_UNIQUE AS SIGNED), CAST(SEQ_IN_INDEX AS SIGNED), INDEX_TYPE \
             FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = COALESCE(?, DATABASE()) AND TABLE_NAME = ? AND COLUMN_NAME IS NOT NULL \
             ORDER BY INDEX_NAME, SEQ_IN_INDEX",
            (schema_owned.clone(), table.to_string()),
        )
        .await
        .map_err(err)?;
    let mut indexes: Vec<IndexInfo> = Vec::new();
    for (name, column, non_unique, _seq, method) in stat_rows {
        match indexes.iter_mut().find(|i| i.name == name) {
            Some(existing) => existing.columns.push(column),
            None => indexes.push(IndexInfo {
                primary: name == "PRIMARY",
                unique: non_unique == 0,
                method: Some(method),
                columns: vec![column],
                name,
            }),
        }
    }
    indexes.sort_by(|a, b| b.primary.cmp(&a.primary).then(a.name.cmp(&b.name)));
    mark_primary_keys(&mut columns, &indexes);

    let fk_rows: Vec<(String, String, String, String, i64, String, String)> = conn
        .exec(
            "SELECT kcu.CONSTRAINT_NAME, kcu.COLUMN_NAME, kcu.REFERENCED_TABLE_NAME, \
                    kcu.REFERENCED_COLUMN_NAME, CAST(kcu.ORDINAL_POSITION AS SIGNED), \
                    rc.UPDATE_RULE, rc.DELETE_RULE \
             FROM information_schema.KEY_COLUMN_USAGE kcu \
             JOIN information_schema.REFERENTIAL_CONSTRAINTS rc \
               ON rc.CONSTRAINT_SCHEMA = kcu.CONSTRAINT_SCHEMA \
              AND rc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME \
              AND rc.TABLE_NAME = kcu.TABLE_NAME \
             WHERE kcu.TABLE_SCHEMA = COALESCE(?, DATABASE()) AND kcu.TABLE_NAME = ? \
               AND kcu.REFERENCED_TABLE_NAME IS NOT NULL \
             ORDER BY kcu.CONSTRAINT_NAME, kcu.ORDINAL_POSITION",
            (schema_owned, table.to_string()),
        )
        .await
        .map_err(err)?;
    let mut foreign_keys: Vec<ForeignKey> = Vec::new();
    for (name, column, ref_table, ref_column, _pos, on_update, on_delete) in fk_rows {
        match foreign_keys
            .iter_mut()
            .find(|f| f.name.as_deref() == Some(name.as_str()))
        {
            Some(existing) => {
                existing.columns.push(column);
                existing.ref_columns.push(ref_column);
            }
            None => foreign_keys.push(ForeignKey {
                name: Some(name),
                columns: vec![column],
                ref_table,
                ref_columns: vec![ref_column],
                on_update: Some(on_update),
                on_delete: Some(on_delete),
            }),
        }
    }

    // `SHOW CREATE TABLE` returns (name, ddl); a view returns more columns and is left without DDL.
    let ddl = conn
        .query_first::<(String, String), _>(format!(
            "SHOW CREATE TABLE {}",
            quote_table(driver, schema, table)
        ))
        .await
        .ok()
        .flatten()
        .map(|(_, ddl)| format!("{ddl};"));

    Ok(TableStructure {
        table: table.into(),
        schema: schema.map(str::to_string),
        columns,
        foreign_keys,
        indexes,
        ddl,
    })
}

// ---------------------------------------------------------------------------------------------
// SQLite
// ---------------------------------------------------------------------------------------------

pub fn structure_sqlite(
    db: &rusqlite::Connection,
    table: &str,
    schema: Option<&str>,
) -> Result<TableStructure, CatableError> {
    let err = |e: rusqlite::Error| fail("sqlite", e);
    let driver = DriverKind::Sqlite;
    let pragma = |name: &str, arg: &str| match schema {
        Some(s) => format!(
            "PRAGMA {}.{name}({})",
            quote_ident(driver, s),
            quote_ident(driver, arg)
        ),
        None => format!("PRAGMA {name}({})", quote_ident(driver, arg)),
    };

    let mut stmt = db.prepare(&pragma("table_info", table)).map_err(err)?;
    let mut columns: Vec<ColumnDetail> = stmt
        .query_map([], |r| {
            Ok(ColumnDetail {
                ordinal: r.get::<_, i64>(0)?.max(0) as u32 + 1,
                name: r.get(1)?,
                data_type: r.get::<_, String>(2)?,
                nullable: r.get::<_, i64>(3)? == 0,
                default: r.get(4)?,
                is_pk: r.get::<_, i64>(5)? > 0,
                is_auto_increment: false,
                comment: None,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    if columns.is_empty() {
        return Err(fail("sqlite", format!("Table '{table}' was not found.")));
    }
    // A lone INTEGER PRIMARY KEY is an alias of the rowid: it numbers itself.
    let pk_count = columns.iter().filter(|c| c.is_pk).count();
    for c in columns.iter_mut() {
        c.is_auto_increment =
            pk_count == 1 && c.is_pk && c.data_type.eq_ignore_ascii_case("INTEGER");
    }

    let mut index_stmt = db.prepare(&pragma("index_list", table)).map_err(err)?;
    let listed: Vec<(String, bool, String)> = index_stmt
        .query_map([], |r| Ok((r.get(1)?, r.get::<_, i64>(2)? != 0, r.get(3)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut indexes: Vec<IndexInfo> = Vec::new();
    for (name, unique, origin) in listed {
        let mut info = db.prepare(&pragma("index_info", &name)).map_err(err)?;
        let cols: Vec<String> = info
            .query_map([], |r| r.get::<_, Option<String>>(2))
            .map_err(err)?
            .filter_map(|c| c.transpose())
            .collect::<Result<_, _>>()
            .map_err(err)?;
        indexes.push(IndexInfo {
            name,
            columns: cols,
            unique,
            primary: origin == "pk",
            method: Some("btree".into()),
        });
    }
    // A rowid-alias primary key has no index entry; still report it.
    if !indexes.iter().any(|i| i.primary) && pk_count > 0 {
        indexes.insert(
            0,
            IndexInfo {
                name: "PRIMARY KEY".into(),
                columns: columns
                    .iter()
                    .filter(|c| c.is_pk)
                    .map(|c| c.name.clone())
                    .collect(),
                unique: true,
                primary: true,
                method: None,
            },
        );
    }
    indexes.sort_by(|a, b| b.primary.cmp(&a.primary).then(a.name.cmp(&b.name)));
    mark_primary_keys(&mut columns, &indexes);

    let mut fk_stmt = db
        .prepare(&pragma("foreign_key_list", table))
        .map_err(err)?;
    let fk_rows: Vec<(i64, String, String, Option<String>, String, String)> = fk_stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut foreign_keys: Vec<(i64, ForeignKey)> = Vec::new();
    for (id, ref_table, from, to, on_update, on_delete) in fk_rows {
        match foreign_keys.iter_mut().find(|(i, _)| *i == id) {
            Some((_, fk)) => {
                fk.columns.push(from);
                fk.ref_columns.extend(to);
            }
            None => foreign_keys.push((
                id,
                ForeignKey {
                    name: None,
                    columns: vec![from],
                    ref_table,
                    ref_columns: to.into_iter().collect(),
                    on_update: Some(on_update),
                    on_delete: Some(on_delete),
                },
            )),
        }
    }

    let master = schema.map_or("sqlite_master".to_string(), |s| {
        format!("{}.sqlite_master", quote_ident(driver, s))
    });
    let mut sql_stmt = db
        .prepare(&format!(
            "SELECT sql FROM {master} WHERE tbl_name = ?1 AND sql IS NOT NULL \
             ORDER BY CASE type WHEN 'table' THEN 0 ELSE 1 END, name"
        ))
        .map_err(err)?;
    let statements: Vec<String> = sql_stmt
        .query_map([table], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let ddl = (!statements.is_empty()).then(|| {
        statements
            .iter()
            .map(|s| format!("{s};"))
            .collect::<Vec<_>>()
            .join("\n")
    });

    Ok(TableStructure {
        table: table.into(),
        schema: schema.map(str::to_string),
        columns,
        foreign_keys: foreign_keys.into_iter().map(|(_, fk)| fk).collect(),
        indexes,
        ddl,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> rusqlite::Connection {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, email TEXT NOT NULL UNIQUE, role TEXT NOT NULL DEFAULT 'member');
             CREATE TABLE orders (
                 id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, status TEXT DEFAULT 'pending',
                 CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE ON UPDATE NO ACTION);
             CREATE INDEX idx_orders_user ON orders (user_id, status);
             CREATE TABLE audit_log (at TEXT, what TEXT);
             CREATE TABLE pair (a INTEGER, b INTEGER, PRIMARY KEY (a, b));",
        )
        .unwrap();
        db
    }

    #[test]
    fn sqlite_columns_defaults_and_auto_increment() {
        let s = structure_sqlite(&db(), "users", None).unwrap();
        let names: Vec<&str> = s.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["id", "email", "role"]);
        let id = &s.columns[0];
        assert!(id.is_pk && id.is_auto_increment);
        let role = &s.columns[2];
        assert_eq!(role.default.as_deref(), Some("'member'"));
        assert!(!role.nullable);
        assert_eq!(s.columns[0].ordinal, 1);
    }

    #[test]
    fn sqlite_foreign_keys_indexes_and_ddl() {
        let s = structure_sqlite(&db(), "orders", None).unwrap();
        assert_eq!(s.foreign_keys.len(), 1);
        let fk = &s.foreign_keys[0];
        assert_eq!(fk.columns, vec!["user_id"]);
        assert_eq!(fk.ref_table, "users");
        assert_eq!(fk.ref_columns, vec!["id"]);
        assert_eq!(fk.on_delete.as_deref(), Some("CASCADE"));

        let idx = s
            .indexes
            .iter()
            .find(|i| i.name == "idx_orders_user")
            .unwrap();
        assert_eq!(idx.columns, vec!["user_id", "status"]);
        assert!(!idx.unique && !idx.primary);
        assert!(
            s.indexes
                .iter()
                .any(|i| i.primary && i.columns == vec!["id"])
        );

        let ddl = s.ddl.unwrap();
        assert!(
            ddl.contains("CREATE TABLE orders") && ddl.contains("fk_user"),
            "{ddl}"
        );
        assert!(ddl.contains("CREATE INDEX idx_orders_user"), "{ddl}");
    }

    #[test]
    fn sqlite_unique_composite_pk_and_no_pk() {
        let users = structure_sqlite(&db(), "users", None).unwrap();
        assert!(
            users
                .indexes
                .iter()
                .any(|i| i.unique && !i.primary && i.columns == vec!["email"])
        );

        let pair = structure_sqlite(&db(), "pair", None).unwrap();
        let pk: Vec<&str> = pair
            .columns
            .iter()
            .filter(|c| c.is_pk)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(pk, vec!["a", "b"]);
        assert!(
            !pair.columns.iter().any(|c| c.is_auto_increment),
            "a composite key does not auto-number"
        );

        let log = structure_sqlite(&db(), "audit_log", None).unwrap();
        assert!(!log.columns.iter().any(|c| c.is_pk));
        assert!(log.indexes.is_empty());
        assert!(log.foreign_keys.is_empty());
    }

    #[test]
    fn a_missing_table_is_an_error_not_an_empty_structure() {
        assert!(structure_sqlite(&db(), "nope", None).is_err());
    }
}
