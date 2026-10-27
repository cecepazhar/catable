use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use ssh2::Session;

use crate::connection::{ConnectionConfig, SshAuthType, SshTunnelConfig};
use crate::error::CatableError;

pub struct TunnelSession {
    local_port: u16,
    shutdown: Arc<AtomicBool>,
}

impl TunnelSession {
    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    pub fn close(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl Drop for TunnelSession {
    fn drop(&mut self) {
        self.close();
    }
}

fn connect_ssh_session(tunnel: &SshTunnelConfig) -> Result<Session, CatableError> {
    let addr = format!("{}:{}", tunnel.host, tunnel.port);
    let socket_addrs = addr
        .to_socket_addrs()
        .map_err(|e| CatableError::Tunnel(format!("Failed to resolve SSH host '{addr}': {e}")))?;

    let mut last_err = None;
    let mut tcp_stream = None;

    for candidate in socket_addrs {
        match TcpStream::connect_timeout(&candidate, Duration::from_secs(10)) {
            Ok(s) => {
                let _ = s.set_read_timeout(Some(Duration::from_secs(15)));
                let _ = s.set_write_timeout(Some(Duration::from_secs(15)));
                tcp_stream = Some(s);
                break;
            }
            Err(e) => last_err = Some(e),
        }
    }

    let tcp = tcp_stream.ok_or_else(|| {
        CatableError::Tunnel(format!(
            "Failed to connect to SSH host '{addr}': {}",
            last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "Connection timed out".into())
        ))
    })?;

    let mut sess = Session::new().map_err(|e| CatableError::Tunnel(e.to_string()))?;
    sess.set_tcp_stream(tcp);
    sess.handshake()
        .map_err(|e| CatableError::Tunnel(format!("SSH handshake failed with '{addr}': {e}")))?;

    match tunnel.auth_type {
        SshAuthType::Password => {
            let pass = tunnel
                .password
                .as_deref()
                .ok_or_else(|| CatableError::Tunnel("Password authentication requires a password".into()))?;
            sess.userauth_password(&tunnel.user, pass)
                .map_err(|e| CatableError::Tunnel(format!("SSH password authentication failed: {e}")))?;
        }
        SshAuthType::PrivateKey => {
            let key_path = tunnel
                .key_path
                .as_deref()
                .filter(|p| !p.trim().is_empty())
                .ok_or_else(|| CatableError::Tunnel("Private key authentication requires a key path".into()))?;

            let expanded_path = if key_path.starts_with("~/") {
                if let Ok(home) = std::env::var("HOME") {
                    Path::new(&home).join(&key_path[2..])
                } else {
                    Path::new(key_path).to_path_buf()
                }
            } else {
                Path::new(key_path).to_path_buf()
            };

            if !expanded_path.exists() {
                return Err(CatableError::Tunnel(format!(
                    "Private key file not found at: {}",
                    expanded_path.display()
                )));
            }

            let passphrase = tunnel.key_passphrase.as_deref();
            sess.userauth_pubkey_file(&tunnel.user, None, &expanded_path, passphrase)
                .map_err(|e| CatableError::Tunnel(format!("SSH private key authentication failed: {e}")))?;
        }
        SshAuthType::Agent => {
            let mut agent = sess
                .agent()
                .map_err(|e| CatableError::Tunnel(format!("Failed to connect to SSH agent: {e}")))?;
            agent
                .connect()
                .map_err(|e| CatableError::Tunnel(format!("SSH agent connection error: {e}")))?;
            agent
                .list_identities()
                .map_err(|e| CatableError::Tunnel(format!("Failed to list SSH agent identities: {e}")))?;

            let mut authed = false;
            for identity in agent.identities().unwrap_or_default() {
                if agent.userauth(&tunnel.user, &identity).is_ok() {
                    authed = true;
                    break;
                }
            }

            if !authed {
                return Err(CatableError::Tunnel(
                    "No valid identity found in SSH agent for user".into(),
                ));
            }
        }
    }

    if !sess.authenticated() {
        return Err(CatableError::Tunnel("SSH authentication was rejected".into()));
    }

    Ok(sess)
}

pub fn test_ssh_tunnel(config: &SshTunnelConfig) -> Result<(), CatableError> {
    let _sess = connect_ssh_session(config)?;
    Ok(())
}

pub fn open_tunnel(conn: &ConnectionConfig) -> Result<TunnelSession, CatableError> {
    let tunnel_cfg = conn
        .ssh_tunnel
        .as_ref()
        .filter(|t| t.enabled)
        .ok_or_else(|| CatableError::Tunnel("SSH tunnel is not enabled for this connection".into()))?;

    let sess = connect_ssh_session(tunnel_cfg)?;
    let sess_arc = Arc::new(Mutex::new(sess));

    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| CatableError::Tunnel(format!("Failed to bind local port: {e}")))?;

    let local_port = listener
        .local_addr()
        .map_err(|e| CatableError::Tunnel(e.to_string()))?
        .port();

    let _ = listener.set_nonblocking(true);

    let shutdown_signal = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown_signal);

    let target_host = if conn.host.trim().is_empty() {
        "127.0.0.1".to_string()
    } else {
        conn.host.clone()
    };
    let target_port = conn.effective_port();

    thread::spawn(move || {
        while !shutdown_clone.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut local_stream, _)) => {
                    let sess_inner = Arc::clone(&sess_arc);
                    let t_host = target_host.clone();
                    let shutdown_worker = Arc::clone(&shutdown_clone);

                    thread::spawn(move || {
                        let mut channel = {
                            let guard = sess_inner.lock();
                            match guard.channel_direct_tcpip(&t_host, target_port, None) {
                                Ok(c) => c,
                                Err(_) => return,
                            }
                        };

                        let _ = local_stream.set_nonblocking(true);
                        let mut buf = [0u8; 8192];

                        while !shutdown_worker.load(Ordering::Relaxed) {
                            let mut active = false;

                            // Local -> Remote
                            match local_stream.read(&mut buf) {
                                Ok(0) => break,
                                Ok(n) => {
                                    active = true;
                                    if channel.write_all(&buf[..n]).is_err() {
                                        break;
                                    }
                                }
                                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                                Err(_) => break,
                            }

                            // Remote -> Local
                            match channel.read(&mut buf) {
                                Ok(0) => break,
                                Ok(n) => {
                                    active = true;
                                    if local_stream.write_all(&buf[..n]).is_err() {
                                        break;
                                    }
                                }
                                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                                Err(_) => break,
                            }

                            if !active {
                                thread::sleep(Duration::from_millis(5));
                            }
                        }
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(15));
                }
                Err(_) => break,
            }
        }
    });

    Ok(TunnelSession {
        local_port,
        shutdown: shutdown_signal,
    })
}
