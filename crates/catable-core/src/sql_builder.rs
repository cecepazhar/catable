//! SQL text builders. The UI never concatenates SQL: identifiers are quoted per
//! driver, values are emitted through one audited literal function, and
//! paging / sorting / filtering are described as data ([`BrowseRequest`]).
//!
//! Why literals instead of bind parameters: PostgreSQL's typed-parameter
//! protocol rejects a text parameter against an integer/uuid/timestamp column,
//! while a quoted literal is coerced to the column type by the server on every
//! driver we support. All values pass through [`quote_literal`], which is
//! covered by the injection tests below.

use serde::{Deserialize, Serialize};

use crate::connection::DriverKind;
use crate::error::CatableError;

/// Hard upper bound of rows per page (matches the executor's row cap).
pub const MAX_PAGE_SIZE: u32 = 10_000;
pub const DEFAULT_PAGE_SIZE: u32 = 200;

fn bad(message: impl Into<String>) -> CatableError {
    CatableError::Driver {
        driver: "sql".into(),
        message: message.into(),
    }
}

fn is_mysql_family(driver: DriverKind) -> bool {
    matches!(driver, DriverKind::Mysql | DriverKind::MariaDb)
}

/// Quotes one identifier: backticks for MySQL/MariaDB, double quotes elsewhere.
pub fn quote_ident(driver: DriverKind, name: &str) -> String {
    if is_mysql_family(driver) {
        format!("`{}`", name.replace('`', "``"))
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// `schema.table` (each part quoted) or just `table`.
pub fn quote_table(driver: DriverKind, schema: Option<&str>, table: &str) -> String {
    match schema.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => format!("{}.{}", quote_ident(driver, s), quote_ident(driver, table)),
        None => quote_ident(driver, table),
    }
}

/// A SQL string literal that is safe on every supported driver.
///
/// * `'` is doubled everywhere.
/// * MySQL/MariaDB treat `\` as an escape by default, so it is doubled there.
/// * PostgreSQL uses `E'…'` when a backslash is present, so the result is the
///   same whether or not `standard_conforming_strings` is on.
/// * NUL bytes are rejected (no driver accepts them in text).
pub fn quote_literal(driver: DriverKind, value: &str) -> Result<String, CatableError> {
    if value.contains('\0') {
        return Err(bad("Value contains a NUL byte."));
    }
    let backslash = value.contains('\\');
    let pg_escape_string = backslash && driver == DriverKind::Postgres;

    let mut out = String::with_capacity(value.len() + 3);
    if pg_escape_string {
        out.push('E');
    }
    out.push('\'');
    for ch in value.chars() {
        match ch {
            '\'' => out.push_str("''"),
            '\\' if pg_escape_string || is_mysql_family(driver) => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('\'');
    Ok(out)
}

fn text_cast(driver: DriverKind, column_sql: &str) -> String {
    match driver {
        DriverKind::Mysql | DriverKind::MariaDb => format!("CAST({column_sql} AS CHAR)"),
        DriverKind::DuckDb => format!("CAST({column_sql} AS VARCHAR)"),
        _ => format!("CAST({column_sql} AS TEXT)"),
    }
}

/// Escapes `%`, `_` and the escape character itself (`!`) for a LIKE pattern.
fn escape_like(value: &str) -> String {
    value
        .replace('!', "!!")
        .replace('%', "!%")
        .replace('_', "!_")
}

// ---------------------------------------------------------------------------
// Request model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    /// `LIKE` with the user's own wildcards.
    Like,
    NotLike,
    /// Case-insensitive substring match (wildcards in the value are literal).
    Contains,
    StartsWith,
    In,
    NotIn,
    IsNull,
    IsNotNull,
    Between,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    pub column: String,
    pub op: FilterOp,
    /// Operand for single-value operators.
    #[serde(default)]
    pub value: Option<String>,
    /// Operands for `in` / `not_in` / `between` (exactly two for `between`).
    #[serde(default)]
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterMode {
    #[default]
    And,
    Or,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBy {
    pub column: String,
    #[serde(default)]
    pub descending: bool,
}

fn default_limit() -> u32 {
    DEFAULT_PAGE_SIZE
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseRequest {
    pub table: String,
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u64,
    #[serde(default)]
    pub order_by: Vec<OrderBy>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub filter_mode: FilterMode,
    /// Quick search: rows where ANY of `search_columns` contains this text (case-insensitive,
    /// wildcards are literal). Combined with the filters using AND.
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub search_columns: Vec<String>,
    /// Also run `COUNT(*)` with the same filters (exact, can be slow on huge tables).
    #[serde(default = "default_true")]
    pub with_total: bool,
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

fn filter_sql(driver: DriverKind, f: &Filter) -> Result<String, CatableError> {
    if f.column.trim().is_empty() {
        return Err(bad("A filter has no column."));
    }
    let col = quote_ident(driver, &f.column);
    let lit = |v: &str| quote_literal(driver, v);
    let single = || -> Result<&str, CatableError> {
        f.value
            .as_deref()
            .ok_or_else(|| bad(format!("The filter on '{}' needs a value.", f.column)))
    };
    let like_kw = |case_insensitive: bool| {
        if case_insensitive && driver == DriverKind::Postgres {
            "ILIKE"
        } else {
            "LIKE"
        }
    };

    match f.op {
        FilterOp::Eq => Ok(format!("{col} = {}", lit(single()?)?)),
        FilterOp::Ne => Ok(format!("{col} <> {}", lit(single()?)?)),
        FilterOp::Lt => Ok(format!("{col} < {}", lit(single()?)?)),
        FilterOp::Lte => Ok(format!("{col} <= {}", lit(single()?)?)),
        FilterOp::Gt => Ok(format!("{col} > {}", lit(single()?)?)),
        FilterOp::Gte => Ok(format!("{col} >= {}", lit(single()?)?)),
        FilterOp::Like => Ok(format!(
            "{} {} {} ESCAPE '!'",
            text_cast(driver, &col),
            like_kw(false),
            lit(single()?)?
        )),
        FilterOp::NotLike => Ok(format!(
            "{} NOT {} {} ESCAPE '!'",
            text_cast(driver, &col),
            like_kw(false),
            lit(single()?)?
        )),
        FilterOp::Contains => Ok(format!(
            "{} {} {} ESCAPE '!'",
            text_cast(driver, &col),
            like_kw(true),
            lit(&format!("%{}%", escape_like(single()?)))?
        )),
        FilterOp::StartsWith => Ok(format!(
            "{} {} {} ESCAPE '!'",
            text_cast(driver, &col),
            like_kw(true),
            lit(&format!("{}%", escape_like(single()?)))?
        )),
        FilterOp::In | FilterOp::NotIn => {
            let negate = f.op == FilterOp::NotIn;
            if f.values.is_empty() {
                // `IN ()` is a syntax error; the empty set matches nothing (or, negated, everything).
                return Ok(if negate {
                    "1 = 1".into()
                } else {
                    "1 = 0".into()
                });
            }
            let list = f
                .values
                .iter()
                .map(|v| lit(v))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            Ok(format!(
                "{col} {}IN ({list})",
                if negate { "NOT " } else { "" }
            ))
        }
        FilterOp::IsNull => Ok(format!("{col} IS NULL")),
        FilterOp::IsNotNull => Ok(format!("{col} IS NOT NULL")),
        FilterOp::Between => {
            if f.values.len() != 2 {
                return Err(bad(format!(
                    "The BETWEEN filter on '{}' needs exactly two values.",
                    f.column
                )));
            }
            Ok(format!(
                "{col} BETWEEN {} AND {}",
                lit(&f.values[0])?,
                lit(&f.values[1])?
            ))
        }
    }
}

/// ` WHERE …` (with a leading space) or an empty string.
pub fn build_where(driver: DriverKind, req: &BrowseRequest) -> Result<String, CatableError> {
    let search = req
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !req.search_columns.is_empty());

    let mut clauses: Vec<String> = Vec::new();

    if !req.filters.is_empty() {
        let parts = req
            .filters
            .iter()
            .map(|f| filter_sql(driver, f))
            .collect::<Result<Vec<_>, _>>()?;
        let joiner = match req.filter_mode {
            FilterMode::And => " AND ",
            FilterMode::Or => " OR ",
        };
        let joined = parts.join(joiner);
        // An OR group must be wrapped as soon as anything is ANDed with it.
        let wrap = parts.len() > 1 && (req.filter_mode == FilterMode::Or || search.is_some());
        clauses.push(if wrap { format!("({joined})") } else { joined });
    }

    if let Some(text) = search {
        let any_column = req
            .search_columns
            .iter()
            .map(|column| {
                filter_sql(
                    driver,
                    &Filter {
                        column: column.clone(),
                        op: FilterOp::Contains,
                        value: Some(text.to_string()),
                        values: vec![],
                    },
                )
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(" OR ");
        clauses.push(format!("({any_column})"));
    }

    if clauses.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!(" WHERE {}", clauses.join(" AND ")))
    }
}

fn build_order_by(driver: DriverKind, req: &BrowseRequest) -> String {
    if req.order_by.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = req
        .order_by
        .iter()
        .filter(|o| !o.column.trim().is_empty())
        .map(|o| {
            format!(
                "{} {}",
                quote_ident(driver, &o.column),
                if o.descending { "DESC" } else { "ASC" }
            )
        })
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ORDER BY {}", parts.join(", "))
    }
}

fn check_table(req: &BrowseRequest) -> Result<(), CatableError> {
    if req.table.trim().is_empty() {
        Err(bad("No table was given."))
    } else {
        Ok(())
    }
}

/// `SELECT * FROM … [WHERE …] [ORDER BY …] LIMIT n OFFSET m`.
pub fn build_select(driver: DriverKind, req: &BrowseRequest) -> Result<String, CatableError> {
    check_table(req)?;
    let limit = req.limit.clamp(1, MAX_PAGE_SIZE);
    Ok(format!(
        "SELECT * FROM {}{}{} LIMIT {} OFFSET {}",
        quote_table(driver, req.schema.as_deref(), &req.table),
        build_where(driver, req)?,
        build_order_by(driver, req),
        limit,
        req.offset
    ))
}

/// `SELECT COUNT(*) FROM … [WHERE …]` with the same filters as [`build_select`].
pub fn build_count(driver: DriverKind, req: &BrowseRequest) -> Result<String, CatableError> {
    check_table(req)?;
    Ok(format!(
        "SELECT COUNT(*) FROM {}{}",
        quote_table(driver, req.schema.as_deref(), &req.table),
        build_where(driver, req)?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [DriverKind; 5] = [
        DriverKind::Postgres,
        DriverKind::Mysql,
        DriverKind::MariaDb,
        DriverKind::Sqlite,
        DriverKind::DuckDb,
    ];

    fn req(table: &str) -> BrowseRequest {
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

    fn filter(column: &str, op: FilterOp, value: Option<&str>) -> Filter {
        Filter {
            column: column.into(),
            op,
            value: value.map(str::to_string),
            values: vec![],
        }
    }

    #[test]
    fn identifiers_are_quoted_per_driver() {
        assert_eq!(quote_ident(DriverKind::Postgres, "users"), "\"users\"");
        assert_eq!(quote_ident(DriverKind::Sqlite, "users"), "\"users\"");
        assert_eq!(quote_ident(DriverKind::Mysql, "users"), "`users`");
        assert_eq!(quote_ident(DriverKind::MariaDb, "users"), "`users`");
    }

    #[test]
    fn awkward_identifiers_cannot_break_out_of_the_quotes() {
        assert_eq!(quote_ident(DriverKind::Postgres, "a\"b"), "\"a\"\"b\"");
        assert_eq!(quote_ident(DriverKind::Mysql, "a`b"), "`a``b`");
        assert_eq!(
            quote_ident(DriverKind::Mysql, "Mixed Case Table"),
            "`Mixed Case Table`"
        );
        assert_eq!(
            quote_ident(DriverKind::Postgres, "x\"; DROP TABLE users; --"),
            "\"x\"\"; DROP TABLE users; --\""
        );
        assert_eq!(
            quote_ident(DriverKind::Sqlite, "tabel_ünï"),
            "\"tabel_ünï\""
        );
    }

    #[test]
    fn schema_qualified_tables() {
        assert_eq!(
            quote_table(DriverKind::Postgres, Some("public"), "Orders"),
            "\"public\".\"Orders\""
        );
        assert_eq!(quote_table(DriverKind::Mysql, Some(" "), "t"), "`t`");
        assert_eq!(quote_table(DriverKind::Mysql, None, "t"), "`t`");
    }

    #[test]
    fn literals_double_quotes_and_handle_backslashes_per_driver() {
        assert_eq!(
            quote_literal(DriverKind::Postgres, "it's").unwrap(),
            "'it''s'"
        );
        assert_eq!(quote_literal(DriverKind::Sqlite, "a\\b").unwrap(), "'a\\b'");
        assert_eq!(
            quote_literal(DriverKind::Mysql, "a\\b").unwrap(),
            "'a\\\\b'"
        );
        assert_eq!(
            quote_literal(DriverKind::Postgres, "a\\b").unwrap(),
            "E'a\\\\b'"
        );
        // The classic MySQL breakout: a trailing backslash must not escape the closing quote.
        assert_eq!(quote_literal(DriverKind::Mysql, "x\\").unwrap(), "'x\\\\'");
        assert!(quote_literal(DriverKind::Postgres, "a\0b").is_err());
    }

    #[test]
    fn injection_payload_stays_inside_the_literal() {
        for driver in ALL {
            let mut r = req("users");
            r.filters = vec![filter("name", FilterOp::Eq, Some("x' OR '1'='1"))];
            let sql = build_select(driver, &r).unwrap();
            assert!(sql.contains("'x'' OR ''1''=''1'"), "{driver:?}: {sql}");
        }
    }

    #[test]
    fn select_shape_and_paging() {
        let mut r = req("users");
        r.offset = 100;
        r.order_by = vec![
            OrderBy {
                column: "name".into(),
                descending: false,
            },
            OrderBy {
                column: "id".into(),
                descending: true,
            },
        ];
        assert_eq!(
            build_select(DriverKind::Mysql, &r).unwrap(),
            "SELECT * FROM `users` ORDER BY `name` ASC, `id` DESC LIMIT 50 OFFSET 100"
        );
        assert_eq!(
            build_select(DriverKind::Postgres, &req("users")).unwrap(),
            "SELECT * FROM \"users\" LIMIT 50 OFFSET 0"
        );
    }

    #[test]
    fn limit_is_clamped() {
        let mut r = req("t");
        r.limit = 0;
        assert!(
            build_select(DriverKind::Sqlite, &r)
                .unwrap()
                .contains("LIMIT 1 ")
        );
        r.limit = 5_000_000;
        assert!(
            build_select(DriverKind::Sqlite, &r)
                .unwrap()
                .contains("LIMIT 10000 ")
        );
    }

    #[test]
    fn comparison_and_null_operators() {
        let mut r = req("t");
        r.filters = vec![
            filter("a", FilterOp::Gte, Some("5")),
            filter("b", FilterOp::Ne, Some("x")),
            filter("c", FilterOp::IsNull, None),
            filter("d", FilterOp::IsNotNull, None),
        ];
        assert_eq!(
            build_where(DriverKind::Postgres, &r).unwrap(),
            " WHERE \"a\" >= '5' AND \"b\" <> 'x' AND \"c\" IS NULL AND \"d\" IS NOT NULL"
        );
        r.filter_mode = FilterMode::Or;
        assert!(
            build_where(DriverKind::Postgres, &r)
                .unwrap()
                .contains(" OR ")
        );
    }

    #[test]
    fn in_between_and_empty_in() {
        let mut r = req("t");
        r.filters = vec![Filter {
            column: "id".into(),
            op: FilterOp::In,
            value: None,
            values: vec!["1".into(), "2".into()],
        }];
        assert_eq!(
            build_where(DriverKind::Sqlite, &r).unwrap(),
            " WHERE \"id\" IN ('1', '2')"
        );

        r.filters[0].values.clear();
        assert_eq!(build_where(DriverKind::Sqlite, &r).unwrap(), " WHERE 1 = 0");
        r.filters[0].op = FilterOp::NotIn;
        assert_eq!(build_where(DriverKind::Sqlite, &r).unwrap(), " WHERE 1 = 1");

        r.filters[0] = Filter {
            column: "n".into(),
            op: FilterOp::Between,
            value: None,
            values: vec!["1".into(), "9".into()],
        };
        assert_eq!(
            build_where(DriverKind::Mysql, &r).unwrap(),
            " WHERE `n` BETWEEN '1' AND '9'"
        );
        r.filters[0].values.pop();
        assert!(build_where(DriverKind::Mysql, &r).is_err());
    }

    #[test]
    fn like_family_casts_to_text_and_escapes_wildcards() {
        let mut r = req("t");
        r.filters = vec![filter("c", FilterOp::Contains, Some("50%_off!"))];
        assert_eq!(
            build_where(DriverKind::Postgres, &r).unwrap(),
            " WHERE CAST(\"c\" AS TEXT) ILIKE '%50!%!_off!!%' ESCAPE '!'"
        );
        assert_eq!(
            build_where(DriverKind::Mysql, &r).unwrap(),
            " WHERE CAST(`c` AS CHAR) LIKE '%50!%!_off!!%' ESCAPE '!'"
        );
        r.filters = vec![filter("c", FilterOp::StartsWith, Some("ab"))];
        assert!(
            build_where(DriverKind::Sqlite, &r)
                .unwrap()
                .contains("LIKE 'ab%' ESCAPE '!'")
        );
        r.filters = vec![filter("c", FilterOp::Like, Some("a_%"))];
        assert!(
            build_where(DriverKind::Sqlite, &r)
                .unwrap()
                .contains("LIKE 'a_%' ESCAPE '!'")
        );
    }

    #[test]
    fn quick_search_matches_any_column_and_is_anded_with_filters() {
        let mut r = req("t");
        r.search = Some("50%_off".into());
        r.search_columns = vec!["a".into(), "b c".into()];
        assert_eq!(
            build_where(DriverKind::Postgres, &r).unwrap(),
            " WHERE (CAST(\"a\" AS TEXT) ILIKE '%50!%!_off%' ESCAPE '!' OR CAST(\"b c\" AS TEXT) ILIKE '%50!%!_off%' ESCAPE '!')"
        );

        // With filters: OR-filters are wrapped so the search ANDs with the whole group.
        r.filters = vec![
            filter("x", FilterOp::Eq, Some("1")),
            filter("y", FilterOp::Eq, Some("2")),
        ];
        r.filter_mode = FilterMode::Or;
        let sql = build_where(DriverKind::Mysql, &r).unwrap();
        assert!(
            sql.starts_with(" WHERE (`x` = '1' OR `y` = '2') AND (CAST(`a` AS CHAR) LIKE"),
            "{sql}"
        );

        // A blank search or no columns adds nothing.
        r.search = Some("   ".into());
        assert!(!build_where(DriverKind::Mysql, &r).unwrap().contains("LIKE"));
        r.search = Some("x".into());
        r.search_columns.clear();
        assert!(!build_where(DriverKind::Mysql, &r).unwrap().contains("LIKE"));
    }

    #[test]
    fn missing_operand_and_empty_table_are_errors() {
        let mut r = req("t");
        r.filters = vec![filter("a", FilterOp::Eq, None)];
        assert!(build_select(DriverKind::Sqlite, &r).is_err());
        assert!(build_select(DriverKind::Sqlite, &req("  ")).is_err());
        assert!(build_count(DriverKind::Sqlite, &req("")).is_err());
    }

    #[test]
    fn count_uses_the_same_filters_but_no_paging() {
        let mut r = req("t");
        r.filters = vec![filter("a", FilterOp::Eq, Some("1"))];
        r.offset = 400;
        assert_eq!(
            build_count(DriverKind::Mysql, &r).unwrap(),
            "SELECT COUNT(*) FROM `t` WHERE `a` = '1'"
        );
    }

    #[test]
    fn deserializes_from_the_ui_payload_with_defaults() {
        let r: BrowseRequest = serde_json::from_str(r#"{"table":"users"}"#).unwrap();
        assert_eq!(r.limit, DEFAULT_PAGE_SIZE);
        assert!(r.with_total);
        assert!(r.filters.is_empty());

        let r: BrowseRequest = serde_json::from_str(
            r#"{"table":"u","filters":[{"column":"id","op":"in","values":["1","2"]}],"order_by":[{"column":"id","descending":true}]}"#,
        )
        .unwrap();
        assert_eq!(r.filters[0].op, FilterOp::In);
        assert!(r.order_by[0].descending);
    }

    /// End-to-end on a real SQLite engine: awkward names + the injection payload from the plan.
    #[test]
    fn runs_against_sqlite_with_awkward_names_and_injection_payloads() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE \"Mixed Case Table\" (id INTEGER PRIMARY KEY, \"Full Name\" TEXT, note TEXT);
             INSERT INTO \"Mixed Case Table\" VALUES (1, 'Ann', '100% sure'), (2, 'Bob', 'x'' OR ''1''=''1'), (3, 'Cy', NULL);",
        )
        .unwrap();
        let count = |sql: String| -> i64 { db.query_row(&sql, [], |r| r.get(0)).unwrap() };

        let mut r = req("Mixed Case Table");
        assert_eq!(count(build_count(DriverKind::Sqlite, &r).unwrap()), 3);

        // The payload is matched literally: it equals exactly the one row that stores it.
        r.filters = vec![filter("note", FilterOp::Eq, Some("x' OR '1'='1"))];
        assert_eq!(count(build_count(DriverKind::Sqlite, &r).unwrap()), 1);

        // …and a value that would match everything if it were spliced into SQL matches nothing.
        r.filters = vec![filter("Full Name", FilterOp::Eq, Some("zzz' OR '1'='1"))];
        assert_eq!(count(build_count(DriverKind::Sqlite, &r).unwrap()), 0);

        // Wildcards inside Contains are literal: '%' only matches a real percent sign.
        r.filters = vec![filter("note", FilterOp::Contains, Some("%"))];
        assert_eq!(count(build_count(DriverKind::Sqlite, &r).unwrap()), 1);

        // The generated SELECT is valid and ordered.
        r.filters.clear();
        r.order_by = vec![OrderBy {
            column: "Full Name".into(),
            descending: true,
        }];
        let sql = build_select(DriverKind::Sqlite, &r).unwrap();
        let first: String = db.query_row(&sql, [], |row| row.get(1)).unwrap();
        assert_eq!(first, "Cy");
    }
}
