//! At-rest protection for the connection vault.
//!
//! * `vault.db` is a SQLCipher database with a random 256-bit key.
//! * That key, and the key that encrypts individual credentials, are never stored in the clear
//!   on Windows: each is sealed with DPAPI, so only the same Windows user on the same machine
//!   can open it. Copying the data folder elsewhere yields nothing readable.
//! * It does **not** protect against software already running as that user.
//! * On other platforms the sealed file holds the key with `0600` permissions.
//!
//! Two rules keep user data safe: an existing key file that cannot be read is an error (a new
//! key is never generated over it), and the plaintext → encrypted migration keeps the original
//! until the encrypted copy has been verified.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use aes_gcm::aead::OsRng;
use aes_gcm::aead::rand_core::RngCore;
use rusqlite::Connection;
use zeroize::Zeroizing;

use crate::error::CatableError;

const DB_FILE: &str = "vault.db";
const DB_KEY_FILE: &str = "vault_dbkey.sealed";
const CREDENTIAL_KEY_FILE: &str = "vault_key.sealed";
const LEGACY_CREDENTIAL_KEY_FILE: &str = "vault_key.bin";
const LOCK_FILE: &str = "vault.lock";
const NEW_SUFFIX: &str = "vault.db.encrypting";
const OLD_SUFFIX: &str = "vault.db.premigration";
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

pub type Key = Zeroizing<[u8; 32]>;

fn err(context: &str, e: impl std::fmt::Display) -> CatableError {
    CatableError::Vault(format!("{context}: {e}"))
}

/// Cross-process lock so two app instances never create keys or migrate at the same time.
/// Released when the returned file is dropped.
fn lock_dir(dir: &Path) -> Result<fs::File, CatableError> {
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join(LOCK_FILE))
        .map_err(|e| err("Cannot open the vault lock file", e))?;
    file.lock().map_err(|e| err("Cannot lock the vault", e))?;
    Ok(file)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CatableError> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| err("Cannot write key file", e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| err("Cannot restrict key file", e))?;
        }
        f.write_all(bytes).map_err(|e| err("Cannot write key file", e))?;
        f.sync_all().map_err(|e| err("Cannot flush key file", e))?;
    }
    fs::rename(&tmp, path).map_err(|e| err("Cannot save key file", e))
}

fn to_key(bytes: &[u8]) -> Result<Key, CatableError> {
    <[u8; 32]>::try_from(bytes)
        .map(Zeroizing::new)
        .map_err(|_| CatableError::Vault("Key has an unexpected length".into()))
}

fn read_sealed(path: &Path) -> Result<Key, CatableError> {
    let sealed = fs::read(path).map_err(|e| err("Cannot read key file", e))?;
    let plain = Zeroizing::new(platform::unseal(&sealed).map_err(|e| {
        CatableError::Vault(format!(
            "Cannot unlock {}: {e}. The vault belongs to another Windows account or computer. \
             Nothing was changed; restore from an encrypted backup if the original is gone.",
            path.display()
        ))
    })?);
    to_key(&plain)
}

/// Seals `key` into `path` and proves it can be read back before returning.
fn store_sealed(path: &Path, key: &Key) -> Result<(), CatableError> {
    let sealed = platform::seal(key.as_ref()).map_err(|e| err("Cannot protect the vault key", e))?;
    write_atomic(path, &sealed)?;
    if read_sealed(path)?.as_ref() != key.as_ref() {
        return Err(CatableError::Vault("Key verification failed".into()));
    }
    Ok(())
}

fn random_key() -> Key {
    let mut key = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(key.as_mut());
    key
}

/// Key for the per-credential AES-256-GCM layer. A legacy `vault_key.bin` (plaintext key) is
/// sealed and then removed, so existing saved passwords stay readable.
pub fn credential_key(dir: &Path) -> Result<Key, CatableError> {
    let sealed = dir.join(CREDENTIAL_KEY_FILE);
    let legacy = dir.join(LEGACY_CREDENTIAL_KEY_FILE);
    if sealed.exists() && !legacy.exists() {
        return read_sealed(&sealed);
    }
    let _lock = lock_dir(dir)?;
    if sealed.exists() {
        let key = read_sealed(&sealed)?;
        // A leftover plaintext copy of the same key must not stay behind.
        if fs::read(&legacy).is_ok_and(|b| b.as_slice() == key.as_ref()) {
            fs::remove_file(&legacy).map_err(|e| err("Cannot remove the old key file", e))?;
        }
        return Ok(key);
    }
    if legacy.exists() {
        let bytes = Zeroizing::new(fs::read(&legacy).map_err(|e| err("Cannot read old key", e))?);
        let key = to_key(&bytes)?;
        store_sealed(&sealed, &key)?;
        fs::remove_file(&legacy).map_err(|e| err("Cannot remove the old key file", e))?;
        return Ok(key);
    }
    let key = random_key();
    store_sealed(&sealed, &key)?;
    Ok(key)
}

fn is_plaintext_db(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 16];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && &head == SQLITE_HEADER
}

/// `x'…'` makes SQLCipher use the bytes as the key itself (no passphrase derivation).
fn key_literal(key: &Key) -> Zeroizing<String> {
    Zeroizing::new(format!("x'{}'", hex::encode(key.as_ref())))
}

fn open_keyed(path: &Path, key: &Key) -> Result<Connection, CatableError> {
    let conn = Connection::open(path).map_err(|e| err("Cannot open the vault", e))?;
    conn.pragma_update(None, "key", key_literal(key).as_str())
        .map_err(|e| err("Cannot apply the vault key", e))?;
    // The first real read is what fails when the key is wrong.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))
        .map_err(|e| err("The vault cannot be decrypted with its key", e))?;
    Ok(conn)
}

/// `(table, row count)` for every user table, to compare a copy with its source.
fn table_counts(conn: &Connection) -> Result<Vec<(String, i64)>, CatableError> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .map_err(|e| err("Cannot list vault tables", e))?;
    let names: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .and_then(|rows| rows.collect())
        .map_err(|e| err("Cannot list vault tables", e))?;
    names
        .into_iter()
        .map(|name| {
            let sql = format!("SELECT count(*) FROM \"{}\"", name.replace('"', "\"\""));
            let n: i64 = conn
                .query_row(&sql, [], |r| r.get(0))
                .map_err(|e| err("Cannot count vault rows", e))?;
            Ok((name, n))
        })
        .collect()
}

fn encrypt_copy(plain: &Path, copy: &Path, key: &Key) -> Result<(), CatableError> {
    let source = Connection::open(plain).map_err(|e| err("Cannot open the old vault", e))?;
    source
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get::<_, i64>(0))
        .map_err(|e| err("Cannot flush the old vault", e))
        .and_then(|busy| {
            if busy == 0 {
                Ok(())
            } else {
                Err(CatableError::Vault(
                    "The vault is in use. Close other CATable windows and start again.".into(),
                ))
            }
        })?;
    let expected = table_counts(&source)?;
    let copy_name = copy.to_string_lossy().to_string();
    source
        .execute(
            "ATTACH DATABASE ?1 AS encrypted KEY ?2",
            rusqlite::params![copy_name, key_literal(key).as_str()],
        )
        .map_err(|e| err("Cannot create the encrypted vault", e))?;
    source
        .query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))
        .map_err(|e| err("Cannot copy into the encrypted vault", e))?;
    source
        .execute_batch("DETACH DATABASE encrypted")
        .map_err(|e| err("Cannot finish the encrypted vault", e))?;
    drop(source);

    if is_plaintext_db(copy) {
        return Err(CatableError::Vault("The copy was not encrypted".into()));
    }
    let check = open_keyed(copy, key)?;
    let integrity: String = check
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|e| err("Integrity check failed", e))?;
    if integrity != "ok" || table_counts(&check)? != expected {
        return Err(CatableError::Vault("The encrypted copy does not match the original".into()));
    }
    Ok(())
}

fn remove_sidecars(dir: &Path) {
    let _ = fs::remove_file(dir.join(format!("{DB_FILE}-wal")));
    let _ = fs::remove_file(dir.join(format!("{DB_FILE}-shm")));
}

fn migrate(dir: &Path, key: &Key) -> Result<(), CatableError> {
    let db = dir.join(DB_FILE);
    let copy = dir.join(NEW_SUFFIX);
    let old = dir.join(OLD_SUFFIX);
    let _ = fs::remove_file(&copy);

    if let Err(e) = encrypt_copy(&db, &copy, key) {
        let _ = fs::remove_file(&copy);
        return Err(e); // the original is untouched
    }

    fs::rename(&db, &old).map_err(|e| err("Cannot set the old vault aside", e))?;
    remove_sidecars(dir);
    let swapped = fs::rename(&copy, &db)
        .map_err(|e| err("Cannot move the encrypted vault into place", e))
        .and_then(|_| open_keyed(&db, key).map(drop));
    if let Err(e) = swapped {
        let _ = fs::remove_file(&db);
        fs::rename(&old, &db).map_err(|r| err("Restore failed; the original is at vault.db.premigration", r))?;
        return Err(e);
    }
    // Only now is the plaintext original removed.
    fs::remove_file(&old).map_err(|e| err("Encrypted, but the old plaintext file could not be removed", e))
}

fn database_key(dir: &Path) -> Result<Key, CatableError> {
    let path = dir.join(DB_KEY_FILE);
    if path.exists() {
        return read_sealed(&path);
    }
    let db = dir.join(DB_FILE);
    let has_data = fs::metadata(&db).is_ok_and(|m| m.len() > 0);
    if has_data && !is_plaintext_db(&db) {
        return Err(CatableError::Vault(format!(
            "{} is encrypted but its key file {DB_KEY_FILE} is missing. Nothing was changed.",
            db.display()
        )));
    }
    let key = random_key();
    store_sealed(&path, &key)?;
    Ok(key)
}

/// Opens `dir/vault.db` encrypted, creating or migrating it as needed. This is the only way
/// the app opens the vault database.
pub fn open_vault_db(dir: &Path) -> Result<Connection, CatableError> {
    let db: PathBuf = dir.join(DB_FILE);
    let conn = {
        let _lock = lock_dir(dir)?;
        // A crash between the two renames of a migration leaves only the original: restore it.
        let old = dir.join(OLD_SUFFIX);
        if old.exists() {
            if db.exists() {
                if !is_plaintext_db(&db) {
                    let _ = fs::remove_file(&old);
                }
            } else {
                fs::rename(&old, &db).map_err(|e| err("Cannot restore the vault", e))?;
            }
        }
        let key = database_key(dir)?;
        if is_plaintext_db(&db) {
            migrate(dir, &key)?;
        }
        open_keyed(&db, &key)?
    };
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| err("Cannot configure the vault", e))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| err("Cannot configure the vault", e))?;
    Ok(conn)
}

/// Removes the vault and its keys. The database goes first: if it cannot be deleted the keys
/// are kept, so it never becomes unreadable by accident.
pub fn destroy(dir: &Path) -> Result<(), CatableError> {
    let db = dir.join(DB_FILE);
    if db.exists() {
        fs::remove_file(&db).map_err(|e| err("Cannot delete the vault (is it open?)", e))?;
    }
    remove_sidecars(dir);
    for name in [DB_KEY_FILE, CREDENTIAL_KEY_FILE, LEGACY_CREDENTIAL_KEY_FILE, OLD_SUFFIX, NEW_SUFFIX] {
        let path = dir.join(name);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| err("Cannot delete vault key material", e))?;
        }
    }
    Ok(())
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    /// Ties sealed blobs to this purpose, so another DPAPI consumer's blob is not accepted.
    const ENTROPY: &[u8] = b"com.fathforce.catable/vault/v1";

    fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr() as *mut u8 }
    }

    /// Copies a DPAPI output buffer, wipes it and frees it.
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        // SAFETY: on success DPAPI returns a LocalAlloc'd buffer of exactly cbData bytes.
        unsafe {
            let slice = std::slice::from_raw_parts_mut(out.pbData, out.cbData as usize);
            let copy = slice.to_vec();
            slice.fill(0);
            LocalFree(out.pbData.cast());
            copy
        }
    }

    pub fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
        let (input, entropy) = (blob(plain), blob(ENTROPY));
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: input blobs point to live slices for the duration of the call.
        let ok = unsafe {
            CryptProtectData(
                &input,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(unsafe { take(out) })
    }

    pub fn unseal(sealed: &[u8]) -> Result<Vec<u8>, String> {
        let (input, entropy) = (blob(sealed), blob(ENTROPY));
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: input blobs point to live slices for the duration of the call.
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(unsafe { take(out) })
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
        Ok(plain.to_vec())
    }
    pub fn unseal(sealed: &[u8]) -> Result<Vec<u8>, String> {
        Ok(sealed.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("catable-secure-{tag}-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    /// A vault as versions ≤ 1.0.11 wrote it: plaintext database in WAL mode, plaintext key.
    fn legacy_vault(dir: &Path) {
        let c = Connection::open(dir.join(DB_FILE)).unwrap();
        c.pragma_update(None, "journal_mode", "WAL").unwrap();
        c.execute_batch(
            "CREATE TABLE connections (id TEXT PRIMARY KEY, host TEXT);
             CREATE TABLE pro_state (k TEXT, v TEXT);
             INSERT INTO connections VALUES ('a', 'secret-host.internal'), ('b', 'other');
             INSERT INTO pro_state VALUES ('x', 'y');",
        )
        .unwrap();
        drop(c);
        fs::write(dir.join(LEGACY_CREDENTIAL_KEY_FILE), [7u8; 32]).unwrap();
    }

    #[test]
    fn new_vault_is_encrypted_and_reopens() {
        let dir = temp_dir("new");
        let c = open_vault_db(&dir).unwrap();
        c.execute_batch("CREATE TABLE t(v TEXT); INSERT INTO t VALUES ('needle-in-vault')").unwrap();
        drop(c);
        let bytes = fs::read(dir.join(DB_FILE)).unwrap();
        assert!(!is_plaintext_db(&dir.join(DB_FILE)));
        assert!(!contains(&bytes, b"needle-in-vault"));
        let c = open_vault_db(&dir).unwrap();
        let v: String = c.query_row("SELECT v FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "needle-in-vault");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn migrates_a_plaintext_vault_without_losing_rows() {
        let dir = temp_dir("migrate");
        legacy_vault(&dir);
        let c = open_vault_db(&dir).unwrap();
        assert_eq!(table_counts(&c).unwrap(), vec![("connections".into(), 2), ("pro_state".into(), 1)]);
        drop(c);
        let bytes = fs::read(dir.join(DB_FILE)).unwrap();
        assert!(!contains(&bytes, b"secret-host.internal"));
        assert!(!dir.join(OLD_SUFFIX).exists() && !dir.join(NEW_SUFFIX).exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_credential_key_is_kept_but_no_longer_stored_in_the_clear() {
        let dir = temp_dir("credkey");
        legacy_vault(&dir);
        assert_eq!(credential_key(&dir).unwrap().as_ref(), &[7u8; 32]);
        assert!(!dir.join(LEGACY_CREDENTIAL_KEY_FILE).exists());
        let sealed = fs::read(dir.join(CREDENTIAL_KEY_FILE)).unwrap();
        #[cfg(windows)]
        assert!(!contains(&sealed, &[7u8; 32]));
        let _ = sealed;
        assert_eq!(credential_key(&dir).unwrap().as_ref(), &[7u8; 32]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_or_damaged_key_is_an_error_and_destroys_nothing() {
        let dir = temp_dir("nokey");
        drop(open_vault_db(&dir).unwrap());
        let before = fs::read(dir.join(DB_FILE)).unwrap();

        let key = fs::read(dir.join(DB_KEY_FILE)).unwrap();
        fs::remove_file(dir.join(DB_KEY_FILE)).unwrap();
        assert!(open_vault_db(&dir).is_err());
        assert!(!dir.join(DB_KEY_FILE).exists(), "no replacement key may appear");

        fs::write(dir.join(DB_KEY_FILE), b"garbage").unwrap();
        assert!(open_vault_db(&dir).is_err());
        assert_eq!(fs::read(dir.join(DB_FILE)).unwrap(), before);

        fs::write(dir.join(DB_KEY_FILE), key).unwrap();
        assert!(open_vault_db(&dir).is_ok());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_interrupted_migration_is_recovered() {
        let dir = temp_dir("crash");
        legacy_vault(&dir);
        fs::rename(dir.join(DB_FILE), dir.join(OLD_SUFFIX)).unwrap();
        let c = open_vault_db(&dir).unwrap();
        assert_eq!(table_counts(&c).unwrap()[0], ("connections".into(), 2));
        let _ = fs::remove_dir_all(dir);
    }
}
