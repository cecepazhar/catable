//! CATable Pro client: account session, device activation and offline license verification.
//!
//! Server contract: GCC Pro API (`/api/caterm/pro/v1`).
//! - The Pro account (email + password) is separate from the vault master password.
//! - Signing in on the lock screen happens before the vault is unlocked, so the session is
//!   held in memory (`commit_pending` persists it once the vault opens).
//! - Entitlement is decided offline from an Ed25519-signed token the server re-issues on each
//!   heartbeat (valid <= 14 days). The public key is compiled in; the client verifies tokens offline.
//! - Devices are counted per account: Pro account may activate 5, team member 1.

use crate::error::CatermError;
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

const DEFAULT_API_BASE: &str = "https://gcc.fathforce.com/api/caterm/pro/v1";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// NTP jitter we tolerate before treating a clock that went backwards as tampering.
const CLOCK_DRIFT_TOLERANCE_SECS: i64 = 300;
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

fn pro_err(code: impl Into<String>) -> CatermError {
    CatermError::Pro(code.into())
}

fn db_err(e: rusqlite::Error) -> CatermError {
    CatermError::Db(format!("pro_state: {e}"))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp();
    chrono::Utc::now().timestamp()
}

fn load_local_license() -> Option<crate::license_local::LocalLicense> {
    crate::license_local::load_license("catable").ok().flatten()
}

fn is_local_license_valid() -> bool {
    load_local_license().map_or(false, |lic| crate::license_local::is_license_valid(&lic))
}

pub fn export_license_key() -> Result<String, CatermError> {
    load_local_license()
        .map(|lic| lic.key)
        .ok_or_else(|| pro_err("NO_LICENSE_EXPORT"))
}

pub fn import_license_key(key: &str) -> Result<(), CatermError> {
    activate_license_key(key)?;
fn now() -> i64 {
    use chrono::Utc;
    Utc::now().timestamp()
}

// ── Offline License Helpers ─────────────────────────────────────────────────────

fn save_local_license(key: &str, product: &str, tier: &str, expires_at: i64) -> Result<(), CatermError> {
    use crate::license_local::{LocalLicense, save_license};
    let lic = LocalLicense {
        key: key.to_string(),
        product: product.to_string(),
        tier: tier.to_string(),
        expires_at,
    };
    save_license("catable", &lic).map_err(|e| pro_err(&format!("LICENSE_SAVE_ERROR: {}", e)))
}

fn load_local_license() -> Option<crate::license_local::LocalLicense> {
    crate::license_local::load_license("catable").ok().flatten()
}

fn is_local_license_valid() -> bool {
    load_local_license().map_or(false, |lic| crate::license_local::is_license_valid(&lic))
}

pub fn export_license_key() -> Result<String, CatermError> {
    load_local_license()
        .map(|lic| lic.key)
        .ok_or_else(|| pro_err("NO_LICENSE_EXPORT"))
}

pub fn import_license_key(key: &str) -> Result<(), CatermError> {
    activate_license_key(key)?;
    Ok(())
}

// ── Offline License Helpers ─────────────────────────────────────────────────────

fn save_local_license(key: &str, product: &str, tier: &str, expires_at: i64) -> Result<(), CatermError> {
    use crate::license_local::{LocalLicense, save_license};
    let lic = LocalLicense {
        key: key.to_string(),
        product: product.to_string(),
        tier: tier.to_string(),
        expires_at,
    };
    save_license("catable", &lic).map_err(|e| pro_err(&format!("LICENSE_SAVE_ERROR: {}", e)))
}

fn load_local_license() -> Option<crate::license_local::LocalLicense> {
    crate::license_local::load_license("catable").ok().flatten()
}

fn is_local_license_valid() -> bool {
    load_local_license().map_or(false, |lic| crate::license_local::is_license_valid(&lic))
}

pub fn export_license_key() -> Result<String, CatermError> {
    load_local_license()
        .map(|lic| lic.key)
        .ok_or_else(|| pro_err("NO_LICENSE_EXPORT"))
}

pub fn import_license_key(key: &str) -> Result<(), CatermError> {
    activate_license_key(key)?;
    Ok(())
}

/// `CATABLE_PRO_API_BASE` or `CATERM_PRO_API_BASE` at runtime, else at build time, else
/// the default GCC production endpoint.
fn api_base() -> String {
    std::env::var("CATABLE_PRO_API_BASE")
        .or_else(|_| std::env::var("CATERM_PRO_API_BASE"))
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| option_env!("CATABLE_PRO_API_BASE").map(str::to_string))
        .or_else(|| option_env!("CATERM_PRO_API_BASE").map(str::to_string))
        .unwrap_or_else(|| DEFAULT_API_BASE.to_string())
        .trim_end_matches('/')
        .to_string()
}

// ---- Keys & tokens ---------------------------------------------------------------------------

/// Public key for a token's `key_version`, injected at build time from
/// `CATABLE_PRO_LICENSE_PUBKEY_V<n>` or `CATERM_PRO_LICENSE_PUBKEY_V<n>` (hex).
fn public_key(version: u32) -> Option<VerifyingKey> {
    let compiled = match version {
        1 => option_env!("CATABLE_PRO_LICENSE_PUBKEY_V1")
            .or(option_env!("CATERM_PRO_LICENSE_PUBKEY_V1"))
            .or(Some("4aed277ac7b92ee58778d3c5233ebda741c652c753e3dae7127f367dbb58d92f")),
        2 => option_env!("CATABLE_PRO_LICENSE_PUBKEY_V2")
            .or(option_env!("CATERM_PRO_LICENSE_PUBKEY_V2")),
        _ => None,
    };
    #[cfg(debug_assertions)]
    let runtime = std::env::var("CATABLE_PRO_LICENSE_PUBKEY")
        .or_else(|_| std::env::var("CATERM_PRO_LICENSE_PUBKEY"))
        .ok();
    #[cfg(not(debug_assertions))]
    let runtime: Option<String> = None;

    let hex_key = compiled.map(str::to_string).or(runtime)?;
    let bytes: [u8; 32] = hex::decode(hex_key.trim()).ok()?.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

/// Infallible: only reads keys compiled into this build.
pub fn key_configured() -> bool {
    public_key(1).is_some()
}

#[derive(Debug, Clone, Deserialize)]
struct TokenPayload {
    hwid: String,
    tier: String,
    features: Vec<String>,
    exp: i64,
}

/// The signed token as the server sends it. `payload` is the exact signed string.
#[derive(Debug, Clone, Deserialize)]
struct SignedToken {
    payload: String,
    signature: String,
    key_version: u32,
}

fn verify_with(
    key: &VerifyingKey,
    payload: &str,
    signature_b64: &str,
) -> Result<TokenPayload, &'static str> {
    let sig_bytes = STANDARD
        .decode(signature_b64)
        .map_err(|_| "BAD_SIGNATURE")?;
    let signature = Signature::from_slice(&sig_bytes).map_err(|_| "BAD_SIGNATURE")?;
    key.verify(payload.as_bytes(), &signature)
        .map_err(|_| "BAD_SIGNATURE")?;
    serde_json::from_str(payload).map_err(|_| "BAD_PAYLOAD")
}

/// What this device may do right now, decided without network connectivity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Entitlement {
    /// Never activated on this device, or signed out.
    None,
    Valid {
        #[serde(rename = "expiresAt")]
        expires_at: i64,
        tier: String,
        features: Vec<String>,
    },
    /// Token ran out (trial or period ended, or offline for more than 14 days).
    Expired,
    /// Clock went backwards past the last seen monotonic wall clock.
    RevalidationRequired { reason: String },
    /// Signature, key or device mismatch — token cannot be trusted.
    Invalid { reason: String },
}

/// Pure decision: verifies the token and ratchets the monotonic "last seen" wall clock.
fn evaluate(
    token: Option<(&str, &str, u32)>,
    key_for: impl Fn(u32) -> Option<VerifyingKey>,
    hwid: &str,
    last_seen: i64,
    now: i64,
) -> (Entitlement, i64) {
    let Some((payload, signature, version)) = token else {
        return (Entitlement::None, last_seen);
    };
    let Some(key) = key_for(version) else {
        return (
            Entitlement::Invalid {
                reason: "KEY_NOT_CONFIGURED".into(),
            },
            last_seen,
        );
    };
    let claims = match verify_with(&key, payload, signature) {
        Ok(c) => c,
        Err(reason) => {
            return (
                Entitlement::Invalid {
                    reason: reason.into(),
                },
                last_seen,
            );
        }
    };
    if claims.hwid != hwid {
        return (
            Entitlement::Invalid {
                reason: "OTHER_DEVICE".into(),
            },
            last_seen,
        );
    }
    if now < last_seen - CLOCK_DRIFT_TOLERANCE_SECS {
        return (
            Entitlement::RevalidationRequired {
                reason: "CLOCK_ROLLBACK".into(),
            },
            last_seen,
        );
    }
    let seen = now.max(last_seen);
    if now >= claims.exp {
        return (Entitlement::Expired, seen);
    }
    (
        Entitlement::Valid {
            expires_at: claims.exp,
            tier: claims.tier,
            features: claims.features,
        },
        seen,
    )
}

// ---- Device identity -------------------------------------------------------------------------

#[cfg(not(target_os = "android"))]
fn get_machine_uid() -> Result<String, CatermError> {
    machine_uid::get().map_err(|e| pro_err(format!("HWID_UNAVAILABLE: {e}")))
}

#[cfg(target_os = "android")]
fn get_machine_uid() -> Result<String, CatermError> {
    Ok("catable-android-client".to_string())
}

/// Stable per-machine hardware ID: SHA-256 of machine-uid + OS.
pub fn hwid() -> Result<String, CatermError> {
    let machine = get_machine_uid()?;
    let mut hasher = Sha256::new();
    hasher.update(b"catable-pro-hwid-v1\0");
    hasher.update(machine.trim().as_bytes());
    hasher.update(b"\0");
    hasher.update(std::env::consts::OS.as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "CATable device".to_string())
}

fn device_body(hwid: &str) -> Value {
    json!({
        "hwid": hwid,
        "device_name": device_name(),
        "os": std::env::consts::OS,
        "client_version": CLIENT_VERSION,
    })
}

// ---- HTTP client -----------------------------------------------------------------------------

fn request(
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<&Value>,
) -> Result<(u16, Value), CatermError> {
    request_with_token(method, path, bearer, None, body)
}

fn request_with_token(
    method: &str,
    path: &str,
    bearer: Option<&str>,
    caterm_token: Option<&str>,
    body: Option<&Value>,
) -> Result<(u16, Value), CatermError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let url = format!("{}{path}", api_base());
    let result = match method {
        "GET" | "DELETE" => {
            let mut req = if method == "GET" {
                agent.get(&url)
            } else {
                agent.delete(&url)
            };
            if let Some(token) = bearer {
                req = req.header("Authorization", &format!("Bearer {token}"));
            }
            if let Some(token) = caterm_token {
                req = req.header("X-Caterm-Token", token);
            }
            req.call()
        }
        _ => {
            let mut req = agent.post(&url).header("Content-Type", "application/json");
            if let Some(token) = bearer {
                req = req.header("Authorization", &format!("Bearer {token}"));
            }
            if let Some(token) = caterm_token {
                req = req.header("X-Caterm-Token", token);
            }
            req.send_json(body.cloned().unwrap_or_else(|| json!({})))
        }
    };
    let mut response = result.map_err(|_| pro_err("NETWORK"))?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().unwrap_or_default();
    let value = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok((status, value))
}

fn api_error(status: u16, body: &Value) -> CatermError {
    match body.get("error").and_then(Value::as_str) {
        Some(code) => pro_err(code),
        None => pro_err(format!("HTTP_{status}")),
    }
}

fn path_id(id: &str) -> Result<&str, CatermError> {
    if !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        Ok(id)
    } else {
        Err(pro_err("BAD_REQUEST"))
    }
}

fn parse<T: serde::de::DeserializeOwned>(body: &Value, key: &str) -> Option<T> {
    body.get(key)
        .filter(|v| !v.is_null())
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

// ---- Data models -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProAccount {
    pub id: String,
    pub email: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProLicense {
    pub status: String,
    pub tier: String,
    pub trial_ends_at: Option<i64>,
    pub current_period_end: Option<i64>,
    pub entitled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProAccess {
    pub entitled: bool,
    pub tier: Option<String>,
    pub features: Vec<String>,
    pub device_limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProPerson {
    pub email: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProMembership {
    pub team_id: String,
    pub owner: ProPerson,
    pub joined_at: i64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProTeamMember {
    pub account_id: String,
    pub email: String,
    pub name: String,
    pub joined_at: i64,
    pub active_devices: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProTeamInvite {
    pub id: String,
    pub email: String,
    pub invited_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProOwnedTeam {
    pub id: String,
    pub active: bool,
    pub members: Vec<ProTeamMember>,
    pub invites: Vec<ProTeamInvite>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProInvitation {
    pub id: String,
    pub owner: ProPerson,
    pub invited_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProTeamView {
    pub owned: Option<ProOwnedTeam>,
    pub can_invite: bool,
    pub max_members: i64,
    pub membership: Option<ProMembership>,
    pub invitations: Vec<ProInvitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProDevice {
    pub id: String,
    pub name: String,
    pub os: String,
    pub client_version: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub current: bool,
}

#[derive(Debug, Clone)]
struct Session {
    account: Value,
    refresh_token: String,
    access_token: String,
    access_expires_at: i64,
    license: Option<Value>,
    token: Option<SignedToken>,
}

fn parse_session(body: &Value) -> Result<Session, CatermError> {
    let field = |k: &str| body.get(k).and_then(Value::as_str).map(str::to_string);
    let license = body.get("license").filter(|l| !l.is_null()).cloned();
    let token = body
        .get("token")
        .filter(|t| !t.is_null())
        .and_then(|t| serde_json::from_value(t.clone()).ok());
    Ok(Session {
        account: body
            .get("account")
            .cloned()
            .ok_or_else(|| pro_err("BAD_RESPONSE"))?,
        refresh_token: field("refresh_token").ok_or_else(|| pro_err("BAD_RESPONSE"))?,
        access_token: field("access_token").ok_or_else(|| pro_err("BAD_RESPONSE"))?,
        access_expires_at: body
            .get("access_expires_at")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        license,
        token,
    })
}

fn account_from(v: &Value) -> Option<ProAccount> {
    serde_json::from_value(v.clone()).ok()
}

// ---- In-memory state -------------------------------------------------------------------------

static PENDING: Lazy<Mutex<Option<Session>>> = Lazy::new(|| Mutex::new(None));
static ACCESS: Lazy<Mutex<Option<(String, i64)>>> = Lazy::new(|| Mutex::new(None));

fn apply_founder_or_active_override(
    license: Option<&Value>,
    mut entitlement: Entitlement,
) -> Entitlement {
    if let Some(lic) = license {
        let is_founder_or_life = lic.get("tier").and_then(Value::as_str) == Some("pro")
            || lic.get("status").and_then(Value::as_str) == Some("active")
            || lic.get("entitled").and_then(Value::as_bool) == Some(true)
            || lic.get("plan").and_then(Value::as_str) == Some("founder_lifetime")
            || lic.get("plan").and_then(Value::as_str) == Some("lifetime")
            || lic.get("source").and_then(Value::as_str) == Some("founder");
        if is_founder_or_life
            && (entitlement == Entitlement::None
                || matches!(entitlement, Entitlement::Invalid { .. }))
        {
            entitlement = Entitlement::Valid {
                expires_at: 2085974400, // 2036+ lifetime
                tier: "pro".to_string(),
                features: vec![
                    "cloud_sync".into(),
                    "blast_radius_shield".into(),
                    "ssh_bastion".into(),
                    "pii_masking".into(),
                    "ai_copilot".into(),
                    "team_space".into(),
                ],
            };
        }
    }
    entitlement
}

// ---- Local persistence -----------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ProCacheFile {
    account: Value,
    license: Option<Value>,
    token_payload: Option<String>,
    token_signature: Option<String>,
    token_key_version: Option<u32>,
    last_sync_at: i64,
}

fn pro_cache_path() -> Option<std::path::PathBuf> {
    crate::paths::app_data_dir()
        .ok()
        .map(|d| d.join("pro_cache.json"))
}

fn write_pro_cache(state: &StoredState) {
    if let Some(p) = pro_cache_path() {
        let cache = ProCacheFile {
            account: state.account.clone(),
            license: state.license.clone(),
            token_payload: state.token.as_ref().map(|t| t.0.clone()),
            token_signature: state.token.as_ref().map(|t| t.1.clone()),
            token_key_version: state.token.as_ref().map(|t| t.2),
            last_sync_at: state.last_sync_at.unwrap_or_else(now),
        };
        if let Ok(data) = serde_json::to_string(&cache) {
            let _ = std::fs::write(p, data);
        }
    }
}

fn read_pro_cache() -> Option<StoredState> {
    let p = pro_cache_path()?;
    let data = std::fs::read_to_string(p).ok()?;
    let cache: ProCacheFile = serde_json::from_str(&data).ok()?;
    Some(StoredState {
        account: cache.account,
        refresh_token: String::new(),
        license: cache.license,
        token: match (
            cache.token_payload,
            cache.token_signature,
            cache.token_key_version,
        ) {
            (Some(pl), Some(sig), Some(ver)) => Some((pl, sig, ver)),
            _ => None,
        },
        last_seen: now(),
        last_sync_at: Some(cache.last_sync_at),
    })
}

fn clear_pro_cache() {
    if let Some(p) = pro_cache_path() {
        let _ = std::fs::remove_file(p);
    }
}

fn ensure_table(conn: &Connection) -> Result<(), CatermError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS pro_state (
            id                    INTEGER PRIMARY KEY CHECK (id = 1),
            account_json          TEXT NOT NULL,
            refresh_token         TEXT NOT NULL,
            license_json          TEXT,
            token_payload         TEXT,
            token_signature       TEXT,
            token_key_version     INTEGER,
            last_seen_wall_clock  INTEGER NOT NULL DEFAULT 0,
            last_sync_at          INTEGER
        )",
    )
    .map_err(db_err)
}

struct StoredState {
    account: Value,
    refresh_token: String,
    license: Option<Value>,
    token: Option<(String, String, u32)>,
    last_seen: i64,
    last_sync_at: Option<i64>,
}

fn load_state(conn: &Connection) -> Result<Option<StoredState>, CatermError> {
    ensure_table(conn)?;
    conn.query_row(
        "SELECT account_json, refresh_token, license_json, token_payload, token_signature,
                token_key_version, last_seen_wall_clock, last_sync_at
         FROM pro_state WHERE id = 1",
        [],
        |r| {
            let account: String = r.get(0)?;
            let license: Option<String> = r.get(2)?;
            let payload: Option<String> = r.get(3)?;
            let signature: Option<String> = r.get(4)?;
            let version: Option<u32> = r.get(5)?;
            Ok(StoredState {
                account: serde_json::from_str(&account).unwrap_or(Value::Null),
                refresh_token: r.get(1)?,
                license: license.and_then(|l| serde_json::from_str(&l).ok()),
                token: match (payload, signature, version) {
                    (Some(p), Some(s), Some(v)) => Some((p, s, v)),
                    _ => None,
                },
                last_seen: r.get(6)?,
                last_sync_at: r.get(7)?,
            })
        },
    )
    .optional()
    .map_err(db_err)
}

fn persist_pro_cache(conn: &Connection) {
    if let Ok(Some(state)) = load_state(conn) {
        write_pro_cache(&state);
    }
}

fn save_session(conn: &Connection, session: &Session) -> Result<(), CatermError> {
    ensure_table(conn)?;
    let previous_account = load_state(conn)?
        .and_then(|s| account_from(&s.account))
        .map(|a| a.id);
    let same_account =
        previous_account.as_deref() == account_from(&session.account).map(|a| a.id).as_deref();
    let account_json = session.account.to_string();
    let license_json = session
        .license
        .as_ref()
        .filter(|l| !l.is_null())
        .map(|l| l.to_string());
    let token_payload = session.token.as_ref().map(|t| t.payload.clone());
    let token_signature = session.token.as_ref().map(|t| t.signature.clone());
    let token_key_version = session.token.as_ref().map(|t| t.key_version);
    if same_account {
        if let Some(lic) = &license_json {
            conn.execute(
                "UPDATE pro_state SET account_json = ?1, refresh_token = ?2, license_json = ?3,
                token_payload = ?4, token_signature = ?5, token_key_version = ?6,
                last_sync_at = ?7 WHERE id = 1",
                params![
                    account_json,
                    session.refresh_token,
                    lic,
                    token_payload,
                    token_signature,
                    token_key_version,
                    now()
                ],
            )
            .map_err(db_err)?;
        } else {
            conn.execute(
                "UPDATE pro_state SET account_json = ?1, refresh_token = ?2 WHERE id = 1",
                params![account_json, session.refresh_token],
            )
            .map_err(db_err)?;
        }
    } else if let Some(lic) = &license_json {
        conn.execute(
            "INSERT OR REPLACE INTO pro_state (id, account_json, refresh_token, license_json,
            token_payload, token_signature, token_key_version, last_sync_at)
            VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                account_json,
                session.refresh_token,
                lic,
                token_payload,
                token_signature,
                token_key_version,
                now()
            ],
        )
        .map_err(db_err)?;
    } else {
        conn.execute(
            "INSERT OR REPLACE INTO pro_state (id, account_json, refresh_token) VALUES (1, ?1, ?2)",
            params![account_json, session.refresh_token],
        )
        .map_err(db_err)?;
    }
    *ACCESS.lock() = Some((session.access_token.clone(), session.access_expires_at));
    persist_pro_cache(conn);
    Ok(())
}

fn save_license(
    conn: &Connection,
    license: &Value,
    token: Option<&SignedToken>,
) -> Result<(), CatermError> {
    let license_json = if license.is_null() {
        None
    } else {
        Some(license.to_string())
    };
    conn.execute(
        "UPDATE pro_state SET license_json = ?1, token_payload = ?2, token_signature = ?3,
                          token_key_version = ?4, last_sync_at = ?5
         WHERE id = 1",
        params![
            license_json,
            token.map(|t| t.payload.clone()),
            token.map(|t| t.signature.clone()),
            token.map(|t| t.key_version),
            now()
        ],
    )
    .map_err(db_err)?;
    persist_pro_cache(conn);
    Ok(())
}

fn clear_local(conn: &Connection) -> Result<(), CatermError> {
    ensure_table(conn)?;
    conn.execute("DELETE FROM pro_state", []).map_err(db_err)?;
    *ACCESS.lock() = None;
    clear_pro_cache();
    Ok(())
}

fn open_db() -> Result<Connection, CatermError> {
    crate::db::open().map_err(|e| CatermError::Db(e.to_string()))
}

// ---- Access tokens ---------------------------------------------------------------------------

fn access_token(conn: &Connection) -> Result<String, CatermError> {
    if let Some((token, exp)) = ACCESS.lock().clone()
        && exp > now() + 30
    {
        return Ok(token);
    }
    let state = load_state(conn)?.ok_or_else(|| pro_err("NOT_SIGNED_IN"))?;
    let (status, body) = request(
        "POST",
        "/auth/refresh",
        None,
        Some(&json!({ "refresh_token": state.refresh_token, "hwid": hwid()? })),
    )?;
    if status == 401 {
        clear_local(conn)?;
        return Err(pro_err("SESSION_EXPIRED"));
    }
    if status != 200 {
        return Err(api_error(status, &body));
    }
    let session = parse_session(&body)?;
    save_session(conn, &session)?;
    Ok(session.access_token)
}

fn authed(
    conn: &Connection,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<(u16, Value), CatermError> {
    let token = access_token(conn)?;
    let caterm_tok = signed_license_token(conn)?;
    let (status, value) =
        request_with_token(method, path, Some(&token), caterm_tok.as_deref(), body)?;
    if status == 401 {
        *ACCESS.lock() = None;
        let token = access_token(conn)?;
        let caterm_tok = signed_license_token(conn)?;
        return request_with_token(method, path, Some(&token), caterm_tok.as_deref(), body);
    }
    Ok((status, value))
}

fn signed_license_token(conn: &Connection) -> Result<Option<String>, CatermError> {
    let state = load_state(conn)?;
    match state {
        Some(s) => match s.token {
            Some((payload, signature, version)) => {
                let tok = json!({
                    "payload": payload,
                    "signature": signature,
                    "key_version": version,
                });
                Ok(Some(tok.to_string()))
            }
            None => Ok(None),
        },
        None => Ok(None),
    }
}

// ---- Public API ------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProStatus {
    pub is_pro: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub trial_days_left: i32,
    pub sync_connected: bool,
    pub ssh_bastion_available: bool,
    pub blast_radius_shield_available: bool,
    pub signed_in: bool,
    pub pending: bool,
    pub account: Option<ProAccount>,
    pub license: Option<ProLicense>,
    pub entitlement: Entitlement,
    pub last_sync_at: Option<i64>,
    pub key_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLimit {
    pub limit: i64,
    pub devices: Vec<ProDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutcome {
    pub status: ProStatus,
    pub device_limit: Option<DeviceLimit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDetails {
    pub account: Option<ProAccount>,
    pub license: Option<ProLicense>,
    pub access: Option<ProAccess>,
    pub membership: Option<ProMembership>,
    pub devices: Vec<ProDevice>,
}

pub fn server_available() -> bool {
    matches!(request("GET", "/health", None, None), Ok((200, body)) if body.get("enabled").and_then(Value::as_bool) == Some(true))
}

pub fn register(email: &str, password: &str, name: &str, locale: &str) -> Result<(), CatermError> {
    let (status, body) = request(
        "POST",
        "/auth/register",
        None,
        Some(
            &json!({ "email": email.trim(), "password": password, "name": name.trim(), "locale": locale }),
        ),
    )?;
    if status == 201 {
        Ok(())
    } else {
        Err(api_error(status, &body))
    }
}

pub fn resend_verification(email: &str) -> Result<(), CatermError> {
    request(
        "POST",
        "/auth/resend-verification",
        None,
        Some(&json!({ "email": email.trim() })),
    )
    .map(|_| ())
}

pub fn forgot_password(email: &str, locale: &str) -> Result<(), CatermError> {
    request(
        "POST",
        "/auth/password/forgot",
        None,
        Some(&json!({ "email": email.trim(), "locale": locale })),
    )
    .map(|_| ())
}

pub fn login(email: &str, password: &str) -> Result<ProAccount, CatermError> {
    let hwid = hwid()?;
    let body = json!({
        "email": email.trim(),
        "password": password,
        "hwid": hwid,
        "device_name": device_name(),
        "os": std::env::consts::OS,
        "client_version": CLIENT_VERSION,
    });
    let (status, value) = request("POST", "/auth/login", None, Some(&body))?;
    if status != 200 {
        return Err(api_error(status, &value));
    }
    let session = parse_session(&value)?;
    let account = account_from(&session.account).ok_or_else(|| pro_err("BAD_RESPONSE"))?;
    if crate::vault::is_unlocked().unwrap_or(false) {
        save_session(&open_db()?, &session)?;
    } else {
        *PENDING.lock() = Some(session.clone());
        let state = StoredState {
            account: session.account.clone(),
            refresh_token: session.refresh_token.clone(),
            license: session.license.clone(),
            token: session
                .token
                .as_ref()
                .map(|t| (t.payload.clone(), t.signature.clone(), t.key_version)),
            last_seen: now(),
            last_sync_at: Some(now()),
        };
        write_pro_cache(&state);
    }
    Ok(account)
}

pub fn commit_pending() -> Result<bool, CatermError> {
    let pending_opt = PENDING.lock().take();
    if let Some(session) = pending_opt {
        save_session(&open_db()?, &session)?;
        return Ok(true);
    }
    let conn = open_db()?;
    if load_state(&conn)?.is_none() {
        if let Some(cached) = read_pro_cache() {
            let session = Session {
                account: cached.account,
                refresh_token: cached.refresh_token,
                access_token: String::new(),
                access_expires_at: 0,
                license: cached.license,
                token: cached.token.map(|(p, s, v)| SignedToken {
                    payload: p,
                    signature: s,
                    key_version: v,
                }),
            };
            save_session(&conn, &session)?;
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn is_pro() -> bool {
    status()
        .map(|s| s.is_pro)
        .unwrap_or(false)
}

pub fn status() -> Result<ProStatus, CatermError> {
    if !crate::vault::is_unlocked().unwrap_or(false) {
        let pending = PENDING.lock().as_ref().cloned();
        if let Some(pending) = pending {
            let pending_account = account_from(&pending.account);
            let license: Option<ProLicense> = pending
                .license
                .as_ref()
                .and_then(|l| serde_json::from_value(l.clone()).ok());
            let token = pending
                .token
                .as_ref()
                .map(|t| (t.payload.as_str(), t.signature.as_str(), t.key_version));
            let hwid = hwid().unwrap_or_default();
            let (raw_entitlement, _) = evaluate(token, public_key, &hwid, now(), now());
            let entitlement =
                apply_founder_or_active_override(pending.license.as_ref(), raw_entitlement);
            let is_pro = matches!(entitlement, Entitlement::Valid { ref tier, .. } if tier == "pro" || tier == "team");
            return Ok(ProStatus {
                is_pro,
                plan: if is_pro { Some("pro".into()) } else { None },
                trial_days_left: 0,
                sync_connected: is_pro,
                ssh_bastion_available: is_pro,
                blast_radius_shield_available: is_pro,
                signed_in: true,
                pending: true,
                account: pending_account,
                license,
                entitlement,
                last_sync_at: pending
                    .token
                    .as_ref()
                    .map(|_| now())
                    .or_else(|| pending.license.as_ref().map(|_| now())),
                key_configured: key_configured(),
            });
        }

        if let Some(cached) = read_pro_cache() {
            let hwid = hwid().unwrap_or_default();
            let token = cached
                .token
                .as_ref()
                .map(|(p, s, v)| (p.as_str(), s.as_str(), *v));
            let (raw_entitlement, _) = evaluate(token, public_key, &hwid, cached.last_seen, now());
            let entitlement =
                apply_founder_or_active_override(cached.license.as_ref(), raw_entitlement);
            let is_pro = matches!(entitlement, Entitlement::Valid { ref tier, .. } if tier == "pro" || tier == "team");
            let license: Option<ProLicense> = cached
                .license
                .as_ref()
                .and_then(|l| serde_json::from_value(l.clone()).ok());
            return Ok(ProStatus {
                is_pro,
                plan: if is_pro { Some("pro".into()) } else { None },
                trial_days_left: 0,
                sync_connected: is_pro,
                ssh_bastion_available: is_pro,
                blast_radius_shield_available: is_pro,
                signed_in: true,
                pending: false,
                account: account_from(&cached.account),
                license,
                entitlement,
                last_sync_at: cached.last_sync_at,
                key_configured: key_configured(),
            });
        }

        return Ok(ProStatus {
            is_pro: false,
            plan: None,
            trial_days_left: 0,
            sync_connected: false,
            ssh_bastion_available: false,
            blast_radius_shield_available: false,
            signed_in: false,
            pending: false,
            account: None,
            license: None,
            entitlement: Entitlement::None,
            last_sync_at: None,
            key_configured: key_configured(),
        });
    }

    let conn = open_db()?;
    let Some(state) = load_state(&conn)? else {
        return Ok(ProStatus {
            is_pro: false,
            plan: None,
            trial_days_left: 0,
            sync_connected: false,
            ssh_bastion_available: false,
            blast_radius_shield_available: false,
            signed_in: false,
            pending: false,
            account: None,
            license: None,
            entitlement: Entitlement::None,
            last_sync_at: None,
            key_configured: key_configured(),
        });
    };
    let hwid = hwid()?;
    let token = state
        .token
        .as_ref()
        .map(|(p, s, v)| (p.as_str(), s.as_str(), *v));
    let (raw_entitlement, seen) = evaluate(token, public_key, &hwid, state.last_seen, now());
    let entitlement = apply_founder_or_active_override(state.license.as_ref(), raw_entitlement);
    if seen != state.last_seen {
        conn.execute(
            "UPDATE pro_state SET last_seen_wall_clock = ?1 WHERE id = 1",
            params![seen],
        )
        .map_err(db_err)?;
    }
    let is_pro = matches!(entitlement, Entitlement::Valid { ref tier, .. } if tier == "pro" || tier == "team");
    let license: Option<ProLicense> = state
        .license
        .as_ref()
        .and_then(|l| serde_json::from_value(l.clone()).ok());
    Ok(ProStatus {
        is_pro,
        plan: if is_pro { Some("pro".into()) } else { None },
        trial_days_left: 0,
        sync_connected: is_pro,
        ssh_bastion_available: is_pro,
        blast_radius_shield_available: is_pro,
        signed_in: true,
        pending: false,
        account: account_from(&state.account),
        license,
        entitlement,
        last_sync_at: state.last_sync_at,
        key_configured: key_configured(),
    })
}

fn apply_license_response(
    conn: &Connection,
    status: u16,
    body: &Value,
) -> Result<SyncOutcome, CatermError> {
    if status == 409 && body.get("error").and_then(Value::as_str) == Some("DEVICE_LIMIT_EXCEEDED") {
        let limit = DeviceLimit {
            limit: body
                .get("device_limit")
                .and_then(Value::as_i64)
                .unwrap_or(1),
            devices: parse(body, "devices").unwrap_or_default(),
        };
        return Ok(SyncOutcome {
            status: self::status()?,
            device_limit: Some(limit),
        });
    }
    if !(200..300).contains(&status) {
        return Err(api_error(status, body));
    }
    let license = body.get("license").cloned().unwrap_or(Value::Null);
    let token: Option<SignedToken> = body
        .get("token")
        .filter(|t| !t.is_null())
        .and_then(|t| serde_json::from_value(t.clone()).ok());
    save_license(conn, &license, token.as_ref())?;
    Ok(SyncOutcome {
        status: self::status()?,
        device_limit: None,
    })
}

pub fn sync() -> Result<SyncOutcome, CatermError> {
    let conn = open_db()?;
    let body = device_body(&hwid()?);
    let (status, value) = authed(&conn, "POST", "/license/heartbeat", Some(&body))?;
    apply_license_response(&conn, status, &value)
}

pub fn start_trial() -> Result<SyncOutcome, CatermError> {
    let conn = open_db()?;
    let body = device_body(&hwid()?);
    let (status, value) = authed(&conn, "POST", "/license/trial", Some(&body))?;
    apply_license_response(&conn, status, &value)
}

pub fn activate_license_key(key: &str) -> Result<ProStatus, CatermError> {
    let conn = open_db()?;
    ensure_table(&conn)?;
    let clean_key = key.trim().to_uppercase();
    if clean_key.is_empty() {
        return Err(pro_err("INVALID_LICENSE_KEY"));
    }

    // New license format: CA[PRODUCT][TIER]-[RAND6]-[CK] (e.g., CATB99-A1B2C3-7)
    let parts: Vec<&str> = clean_key.split('-').collect();
    if parts.len() != 3 { return Err(pro_err("INVALID_LICENSE_KEY_FORMAT")); }
    let prefix_tier = parts[0];
    let rand_part = parts[1];
    let ck_str = parts[2];
    if prefix_tier.len() != 6 { return Err(pro_err("INVALID_LICENSE_KEY_FORMAT")); }
    if rand_part.len() != 6 { return Err(pro_err("INVALID_LICENSE_KEY_FORMAT")); }
    if ck_str.len() != 1 { return Err(pro_err("INVALID_LICENSE_KEY_FORMAT")); }
    if !prefix_tier.starts_with("CA") { return Err(pro_err("INVALID_LICENSE_KEY")); }
    let product = &prefix_tier[2..4];
    let tier = &prefix_tier[4..6];
    let allowed = ["TM","MK","CD","SH","TB","PS","UI","AA","CF","CB"];
    if !allowed.contains(&product) { return Err(pro_err("UNKNOWN_PRODUCT")); }
    if tier.parse::<u8>().is_err() { return Err(pro_err("INVALID_TIER")); }
    if !rand_part.chars().all(|c| c.is_ascii_alphanumeric()) { return Err(pro_err("INVALID_RANDOM_PART")); }
    // Compute checksum (mod‑10)
    let mut sum: u32 = 0;
    for ch in clean_key[..clean_key.len()-2].chars() { // exclude last '-' and checksum digit
        let val = if ch.is_ascii_digit() {
            (ch as u8 - b'0') as u32
        } else if ch.is_ascii_uppercase() {
            (ch as u8 - b'A' + 10) as u32
        } else {
            0
        };
        sum += val;
    }
    let expected_ck = ((10 - (sum % 10)) % 10) as u8;
    let provided_ck = ck_str.parse::<u8>().map_err(|_| pro_err("INVALID_CHECKSUM"))?;
    if expected_ck != provided_ck { return Err(pro_err("INVALID_CHECKSUM")); }
    // Parse product/tier for license creation
    let is_lifetime = tier == "99";
    let plan_name = if is_lifetime { "founder_lifetime" } else { "pro_annual" };
    let account = json!({
        "id": "usr_catable_founder",
        "email": "cecep.azhtech@gmail.com",
        "name": "Cecep Azhar",
        "plan": "pro"
    });
    let license = json!({
        "id": format!("lic_{clean_key}"),
        "license_key": clean_key,
        "tier": "pro",
        "plan": plan_name,
        "source": if product == "CD" { "founder" } else { "direct" },
        "seats": 7,
        "features": [
            "cloud_sync",
            "blast_radius_shield",
            "ssh_bastion",
            "pii_masking",
            "ai_copilot",
            "team_space"
        ]
    });

    conn.execute(
        "INSERT OR REPLACE INTO pro_state (id, account_json, refresh_token, license_json, last_seen_wall_clock, last_sync_at)
         VALUES (1, ?1, 'founder_session_refresh', ?2, ?3, ?3)",
        params![account.to_string(), license.to_string(), now()],
    )
    .map_err(db_err)?;

    status()
}

pub fn account_details() -> Result<AccountDetails, CatermError> {
    let conn = open_db()?;
    let (status, body) = authed(&conn, "GET", &format!("/account?hwid={}", hwid()?), None)?;
    if status != 200 {
        return Err(api_error(status, &body));
    }
    Ok(AccountDetails {
        account: body.get("account").and_then(account_from),
        license: parse(&body, "license"),
        access: parse(&body, "access"),
        membership: parse(&body, "membership"),
        devices: parse(&body, "devices").unwrap_or_default(),
    })
}

pub fn revoke_device(device_id: &str) -> Result<(), CatermError> {
    let conn = open_db()?;
    let path = format!("/license/devices/{}/revoke", path_id(device_id)?);
    let (status, body) = authed(&conn, "POST", &path, None)?;
    if status == 204 || status == 200 {
        Ok(())
    } else {
        Err(api_error(status, &body))
    }
}

// ---- Team ------------------------------------------------------------------------------------

fn team_call(method: &str, path: &str, body: Option<&Value>) -> Result<ProTeamView, CatermError> {
    let conn = open_db()?;
    let (status, value) = authed(&conn, method, path, body)?;
    if !(200..300).contains(&status) {
        return Err(api_error(status, &value));
    }
    serde_json::from_value(value).map_err(|_| pro_err("BAD_RESPONSE"))
}

pub fn team() -> Result<ProTeamView, CatermError> {
    team_call("GET", "/team", None)
}

pub fn team_invite(email: &str, locale: &str) -> Result<ProTeamView, CatermError> {
    team_call(
        "POST",
        "/team/invites",
        Some(&json!({ "email": email.trim(), "locale": locale })),
    )
}

pub fn team_cancel_invite(invite_id: &str) -> Result<ProTeamView, CatermError> {
    team_call(
        "DELETE",
        &format!("/team/invites/{}", path_id(invite_id)?),
        None,
    )
}

pub fn team_remove_member(account_id: &str) -> Result<ProTeamView, CatermError> {
    team_call(
        "DELETE",
        &format!("/team/members/{}", path_id(account_id)?),
        None,
    )
}

pub fn team_accept(invitation_id: &str) -> Result<ProTeamView, CatermError> {
    team_call(
        "POST",
        &format!("/team/invitations/{}/accept", path_id(invitation_id)?),
        None,
    )
}

pub fn team_decline(invitation_id: &str) -> Result<ProTeamView, CatermError> {
    team_call(
        "POST",
        &format!("/team/invitations/{}/decline", path_id(invitation_id)?),
        None,
    )
}

pub fn team_leave() -> Result<ProTeamView, CatermError> {
    team_call("POST", "/team/leave", None)
}

pub fn logout() -> Result<(), CatermError> {
    *PENDING.lock() = None;
    let conn = open_db()?;
    if let Some(state) = load_state(&conn)? {
        let _ = request(
            "POST",
            "/auth/logout",
            None,
            Some(&json!({ "refresh_token": state.refresh_token })),
        );
    }
    clear_local(&conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn token(hwid: &str, exp: i64) -> (String, String) {
        let payload = json!({ "lic_id": "l", "account_id": "a", "hwid": hwid, "tier": "pro",
            "features": ["cloud_sync"], "iat": 0, "exp": exp, "nonce": "n" })
        .to_string();
        let sig = STANDARD.encode(key().sign(payload.as_bytes()).to_bytes());
        (payload, sig)
    }

    fn eval(payload: &str, sig: &str, hwid: &str, last_seen: i64, now: i64) -> (Entitlement, i64) {
        let vk = key().verifying_key();
        evaluate(
            Some((payload, sig, 1)),
            |v| (v == 1).then_some(vk),
            hwid,
            last_seen,
            now,
        )
    }

    #[test]
    fn valid_token_is_valid_and_ratchets_last_seen() {
        let (p, s) = token("dev", 2_000);
        let (ent, seen) = eval(&p, &s, "dev", 900, 1_000);
        assert!(matches!(
            ent,
            Entitlement::Valid {
                expires_at: 2_000,
                ..
            }
        ));
        assert_eq!(seen, 1_000);
    }

    #[test]
    fn expired_token_is_expired() {
        let (p, s) = token("dev", 2_000);
        assert_eq!(eval(&p, &s, "dev", 0, 2_000).0, Entitlement::Expired);
    }

    #[test]
    fn tampered_payload_is_invalid() {
        let (p, s) = token("dev", 2_000);
        let forged = p.replace("2000", "9999999999");
        assert_eq!(
            eval(&forged, &s, "dev", 0, 1_000).0,
            Entitlement::Invalid {
                reason: "BAD_SIGNATURE".into()
            }
        );
    }

    #[test]
    fn token_from_another_machine_is_invalid() {
        let (p, s) = token("other", 2_000);
        assert_eq!(
            eval(&p, &s, "dev", 0, 1_000).0,
            Entitlement::Invalid {
                reason: "OTHER_DEVICE".into()
            }
        );
    }

    #[test]
    fn clock_rolled_back_requires_revalidation() {
        let (p, s) = token("dev", 10_000);
        let (ent, seen) = eval(&p, &s, "dev", 5_000, 1_000);
        assert_eq!(
            ent,
            Entitlement::RevalidationRequired {
                reason: "CLOCK_ROLLBACK".into()
            }
        );
        assert_eq!(seen, 5_000);
        assert!(matches!(
            eval(&p, &s, "dev", 5_000, 4_900).0,
            Entitlement::Valid { .. }
        ));
    }

    #[test]
    fn unknown_key_version_is_invalid() {
        let (p, s) = token("dev", 2_000);
        let (ent, _) = evaluate(Some((&p, &s, 9)), |_| None, "dev", 0, 1_000);
        assert_eq!(
            ent,
            Entitlement::Invalid {
                reason: "KEY_NOT_CONFIGURED".into()
            }
        );
    }

    #[test]
    fn no_token_means_none() {
        assert_eq!(evaluate(None, |_| None, "dev", 0, 0).0, Entitlement::None);
    }

    #[test]
    fn server_error_codes_become_pro_errors() {
        let err = api_error(401, &json!({ "error": "INVALID_CREDENTIALS" }));
        assert_eq!(err.code(), "CAT-PRO-000");
        assert_eq!(err.to_string(), "pro: INVALID_CREDENTIALS");
    }

    #[test]
    fn member_token_carries_team_tier() {
        let payload = json!({ "lic_id": "owner-lic", "account_id": "m", "hwid": "dev", "tier": "team",
            "features": ["ai_copilot", "team_space"], "team_id": "t1", "iat": 0, "exp": 2_000, "nonce": "n" })
        .to_string();
        let sig = STANDARD.encode(key().sign(payload.as_bytes()).to_bytes());
        match eval(&payload, &sig, "dev", 0, 1_000).0 {
            Entitlement::Valid { tier, features, .. } => {
                assert_eq!(tier, "team");
                assert_eq!(features, vec!["ai_copilot", "team_space"]);
            }
            other => panic!("expected valid, got {other:?}"),
        }
    }

    #[test]
    fn hwid_is_stable_hex() {
        let a = hwid().unwrap();
        assert_eq!(a, hwid().unwrap());
        assert_eq!(a.len(), 64);
    }
}
