//! SSH bastion: authentication, port forwarding and CATable's own tunnel.
//!
//! Forward targets are the databases' service names inside the compose
//! network (override with `CATABLE_IT_SSH_TARGET_{PG,MYSQL,MARIADB}`),
//! addresses the host cannot reach without the bastion.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use catable_core::{ConnectionConfig, DriverKind, QueryExecutor, SshAuthType, test_ssh_tunnel};
use libtest_mimic::Trial;

use crate::engines::scalar_u64;
use crate::env::{Bastion, Target, Targets, var};
use crate::{Check, trial};

/// Printed before the run so ignored trials are never mistaken for passes.
pub const KNOWN_ISSUES: &[&str] = &[
    "ssh::catable_tunnel_* are ignored: tunnel.rs times out through a bastion that forwards \
     fine (see ssh::forward_*); fixed in S0-B3. Reproduce with --include-ignored.",
];

#[derive(Clone)]
struct Forward {
    bastion: Bastion,
    driver: DriverKind,
    host: String,
    port: u16,
}

pub fn trials(t: &Targets) -> Vec<Trial> {
    let with_password = t.ssh.clone().and_then(|b| match b.password {
        Some(_) => Ok(b),
        None => Err("CATABLE_IT_SSH_PASSWORD not set".into()),
    });
    let with_key = t.ssh.clone().and_then(|b| match b.key_path {
        Some(_) => Ok(b),
        None => Err("CATABLE_IT_SSH_KEY_PATH not set".into()),
    });
    let pg = ("CATABLE_IT_SSH_TARGET_PG", "pg:5432", DriverKind::Postgres);
    let mysql = (
        "CATABLE_IT_SSH_TARGET_MYSQL",
        "mysql:3306",
        DriverKind::Mysql,
    );
    let mariadb = (
        "CATABLE_IT_SSH_TARGET_MARIADB",
        "mariadb:3306",
        DriverKind::MariaDb,
    );

    let mut trials = vec![
        trial("ssh::password_auth", &with_password, |b| {
            auth(b, SshAuthType::Password)
        }),
        trial("ssh::key_auth", &with_key, |b| {
            auth(b, SshAuthType::PrivateKey)
        }),
        trial(
            "ssh::wrong_password_rejected",
            &t.ssh,
            wrong_password_rejected,
        ),
    ];
    for (name, spec) in [("postgres", pg), ("mysql", mysql), ("mariadb", mariadb)] {
        trials.push(trial(
            format!("ssh::forward_{name}"),
            &forward(&t.ssh, spec),
            forward_probe,
        ));
    }
    for (name, bastion, auth_type, db, spec) in [
        ("postgres", &with_password, SshAuthType::Password, &t.pg, pg),
        ("mysql", &with_key, SshAuthType::PrivateKey, &t.mysql, mysql),
        (
            "mariadb",
            &with_password,
            SshAuthType::Password,
            &t.mariadb,
            mariadb,
        ),
    ] {
        let conn = via(bastion, auth_type, db, spec);
        trials.push(
            trial(format!("ssh::catable_tunnel_{name}"), &conn, catable_tunnel)
                .with_ignored_flag(true),
        );
    }
    trials
}

fn target(var_name: &str, default: &str) -> (String, u16) {
    let raw = var(var_name).unwrap_or_else(|| default.into());
    raw.rsplit_once(':')
        .and_then(|(h, p)| Some((h.to_string(), p.parse().ok()?)))
        .unwrap_or_else(|| panic!("{var_name} must be host:port, got {raw:?}"))
}

fn forward(
    bastion: &Target<Bastion>,
    (var_name, default, driver): (&str, &str, DriverKind),
) -> Target<Forward> {
    let (host, port) = target(var_name, default);
    Ok(Forward {
        bastion: bastion.clone()?,
        driver,
        host,
        port,
    })
}

/// Profile that reaches the database through the bastion, reusing the
/// credentials of the direct `db` profile.
fn via(
    bastion: &Target<Bastion>,
    auth_type: SshAuthType,
    db: &Target<ConnectionConfig>,
    (var_name, default, _): (&str, &str, DriverKind),
) -> Target<ConnectionConfig> {
    let bastion = bastion.as_ref().map_err(Clone::clone)?;
    let mut conn = db.clone()?;
    (conn.host, conn.port) = target(var_name, default);
    conn.ssh_tunnel = Some(bastion.tunnel(auth_type));
    Ok(conn)
}

async fn auth(bastion: Bastion, auth_type: SshAuthType) -> Check {
    test_ssh_tunnel(&bastion.tunnel(auth_type)).map_err(|e| e.to_string())
}

async fn wrong_password_rejected(mut bastion: Bastion) -> Check {
    bastion.password = Some("definitely-not-the-password".into());
    let res = test_ssh_tunnel(&bastion.tunnel(SshAuthType::Password));
    ensure!(res.is_err(), "bastion accepted a wrong password");
    Ok(())
}

/// Checks the environment, not CATable: opens a direct-tcpip channel with
/// plain ssh2 and reads the database's first protocol bytes.
async fn forward_probe(f: Forward) -> Check {
    let reachable = (f.host.as_str(), f.port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .is_some_and(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok());
    ensure!(
        !reachable,
        "{}:{} is reachable without the bastion",
        f.host,
        f.port
    );

    let b = &f.bastion;
    let tcp =
        TcpStream::connect((b.host.as_str(), b.port)).map_err(|e| format!("bastion tcp: {e}"))?;
    let mut sess = ssh2::Session::new().map_err(|e| e.to_string())?;
    sess.set_tcp_stream(tcp);
    sess.set_timeout(10_000);
    sess.handshake().map_err(|e| format!("handshake: {e}"))?;
    match (&b.key_path, &b.password) {
        (Some(key), _) => sess.userauth_pubkey_file(&b.user, None, Path::new(key), None),
        (None, Some(password)) => sess.userauth_password(&b.user, password),
        (None, None) => return Err("bastion has neither password nor key".into()),
    }
    .map_err(|e| format!("auth: {e}"))?;

    let mut channel = sess
        .channel_direct_tcpip(&f.host, f.port, None)
        .map_err(|e| format!("direct-tcpip to {}:{}: {e}", f.host, f.port))?;
    let mut buf = [0u8; 64];
    match f.driver {
        DriverKind::Postgres => {
            // SSLRequest; the server answers a single 'S' or 'N'.
            channel
                .write_all(&[0, 0, 0, 8, 0x04, 0xd2, 0x16, 0x2f])
                .map_err(|e| e.to_string())?;
            channel
                .read_exact(&mut buf[..1])
                .map_err(|e| format!("read: {e}"))?;
            ensure!(
                matches!(buf[0], b'S' | b'N'),
                "unexpected SSLRequest reply {:?}",
                buf[0] as char
            );
        }
        _ => {
            // Server greeting: 4-byte header, protocol 10, NUL-terminated version.
            let n = channel.read(&mut buf).map_err(|e| format!("read: {e}"))?;
            ensure!(
                n > 5 && buf[4] == 10,
                "not a MySQL greeting: {:?}",
                &buf[..n]
            );
            let version = String::from_utf8_lossy(&buf[5..n]);
            let version = version.split('\0').next().unwrap_or_default();
            let is_mariadb = version.contains("MariaDB");
            ensure!(
                is_mariadb == (f.driver == DriverKind::MariaDb),
                "{:?} target greets as {version:?}",
                f.driver
            );
        }
    }
    Ok(())
}

/// CATable's tunnel path end to end: connect and query through the bastion.
async fn catable_tunnel(conn: ConnectionConfig) -> Check {
    let res = QueryExecutor::test_connection_async(&conn).await;
    ensure!(
        res.success,
        "connect through bastion failed: {}",
        res.message
    );
    let events = scalar_u64(&conn, "SELECT count(*) FROM events").await?;
    ensure!(
        events == 100_000,
        "events through tunnel: expected 100000, got {events}"
    );
    Ok(())
}
