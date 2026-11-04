//! Reads the `CATABLE_IT_*` environment into connection profiles.

use catable_core::{ConnectionConfig, DriverKind, Environment, SshAuthType, SshTunnelConfig};
use percent_encoding::percent_decode_str;

/// `Ok` = the server is available; `Err` = why its tests are skipped.
pub type Target<T> = Result<T, String>;

#[derive(Clone)]
pub struct Bastion {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub key_path: Option<String>,
}

impl Bastion {
    pub fn tunnel(&self, auth_type: SshAuthType) -> SshTunnelConfig {
        SshTunnelConfig {
            enabled: true,
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            auth_type,
            key_path: self.key_path.clone(),
            key_passphrase: None,
            password: self.password.clone(),
        }
    }
}

pub struct Targets {
    pub pg: Target<ConnectionConfig>,
    pub mysql: Target<ConnectionConfig>,
    pub mariadb: Target<ConnectionConfig>,
    pub sqlite: Target<ConnectionConfig>,
    pub ssh: Target<Bastion>,
}

impl Targets {
    pub fn from_env() -> Self {
        Self {
            pg: db_from_env("CATABLE_IT_PG_URL", DriverKind::Postgres),
            mysql: db_from_env("CATABLE_IT_MYSQL_URL", DriverKind::Mysql),
            mariadb: db_from_env("CATABLE_IT_MARIADB_URL", DriverKind::MariaDb),
            sqlite: Err("not seeded".into()),
            ssh: bastion_from_env(),
        }
    }

    pub fn print_summary(&self) {
        eprintln!("CATable integration targets:");
        for (name, target) in [
            ("postgres", &self.pg),
            ("mysql", &self.mysql),
            ("mariadb", &self.mariadb),
            ("sqlite", &self.sqlite),
        ] {
            match target {
                Ok(c) if c.driver == DriverKind::Sqlite => eprintln!("  {name:<9}{}", c.database),
                Ok(c) => eprintln!("  {name:<9}{}@{}:{}/{}", c.user, c.host, c.port, c.database),
                Err(reason) => eprintln!("  {name:<9}SKIPPED — {reason}"),
            }
        }
        match &self.ssh {
            Ok(b) => {
                let auth: Vec<&str> = [
                    b.password.as_ref().map(|_| "password"),
                    b.key_path.as_ref().map(|_| "key"),
                ]
                .into_iter()
                .flatten()
                .collect();
                eprintln!(
                    "  {:<9}{}@{}:{} ({})",
                    "ssh",
                    b.user,
                    b.host,
                    b.port,
                    auth.join(", ")
                );
            }
            Err(reason) => eprintln!("  {:<9}SKIPPED — {reason}", "ssh"),
        }
        eprintln!();
    }
}

pub fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn db_from_env(name: &str, driver: DriverKind) -> Target<ConnectionConfig> {
    let raw = var(name).ok_or_else(|| format!("{name} not set"))?;
    // A malformed URL is a configuration error, not a reason to skip.
    Ok(parse_db_url(&raw, driver).unwrap_or_else(|e| panic!("{name} is invalid: {e}")))
}

fn parse_db_url(raw: &str, driver: DriverKind) -> Result<ConnectionConfig, String> {
    let url = url::Url::parse(raw).map_err(|e| e.to_string())?;
    let schemes: &[&str] = match driver {
        DriverKind::Postgres => &["postgres", "postgresql"],
        _ => &["mysql", "mariadb"],
    };
    if !schemes.contains(&url.scheme()) {
        return Err(format!(
            "expected scheme {schemes:?}, got {:?}",
            url.scheme()
        ));
    }
    let decode = |s: &str| percent_decode_str(s).decode_utf8_lossy().into_owned();

    Ok(ConnectionConfig {
        connect_timeout_secs: None,
        id: format!("it-{driver:?}").to_lowercase(),
        name: format!("IT {driver:?}"),
        driver,
        environment: Environment::Development,
        host: url.host_str().ok_or("missing host")?.to_string(),
        port: url.port().unwrap_or(0),
        database: decode(url.path().trim_start_matches('/')),
        user: decode(url.username()),
        password: url.password().map(decode),
        read_only: false,
        ssh_tunnel: None,
        group_name: None,
        workspace_id: None,
    })
}

fn bastion_from_env() -> Target<Bastion> {
    let host = var("CATABLE_IT_SSH_HOST").ok_or("CATABLE_IT_SSH_HOST not set")?;
    let user = var("CATABLE_IT_SSH_USER").ok_or("CATABLE_IT_SSH_USER not set")?;
    let port = match var("CATABLE_IT_SSH_PORT") {
        Some(p) => p
            .parse()
            .unwrap_or_else(|_| panic!("CATABLE_IT_SSH_PORT is invalid: {p:?}")),
        None => 22,
    };
    Ok(Bastion {
        host,
        port,
        user,
        password: var("CATABLE_IT_SSH_PASSWORD"),
        key_path: var("CATABLE_IT_SSH_KEY_PATH"),
    })
}
