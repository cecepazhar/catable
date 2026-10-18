use crate::connection::DriverKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Browse,
    InlineEdit,
    SqlEditor,
    CancelQuery,
    ExplainVisual,
    Erd,
    AlterTable,
    SshTunnel,
    ReadOnlyDriverMode,
}

pub struct DriverRegistry;

impl DriverRegistry {
    /// Returns a human-readable status string describing which capabilities
    /// are fully supported by a given driver kind, used to disable UI
    /// features that would fail at runtime (PRD §2.3 capability matrix).
    pub fn capabilities(driver: DriverKind) -> &'static str {
        match driver {
            DriverKind::Sqlite | DriverKind::Postgres | DriverKind::Mysql | DriverKind::MariaDb => {
                "browse,inline_edit,sql_editor,cancel_query,explain_visual,erd,alter_table,read_only_mode"
            }
            DriverKind::DuckDb => "browse,sql_editor,explain_visual,read_only_mode",
            DriverKind::Redis => "browse,sql_editor,ssh_tunnel",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_gets_full_capability_set() {
        let caps = DriverRegistry::capabilities(DriverKind::Postgres);
        assert!(caps.contains("browse"));
        assert!(caps.contains("erd"));
    }

    #[test]
    fn duckdb_does_not_get_erd() {
        let caps = DriverRegistry::capabilities(DriverKind::DuckDb);
        assert!(!caps.contains("erd"));
    }
}
