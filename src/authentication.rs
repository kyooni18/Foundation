//! Local credentials. Raw tokens have no Debug/Serialize implementation.
use crate::domain::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use subtle::ConstantTimeEq;

pub struct Token(String);
impl Token {
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = secure_read(path.as_ref())?;
        let value = std::str::from_utf8(&bytes)
            .map_err(|_| Error::Credentials)?
            .trim_end_matches('\n');
        if !valid_token(value) {
            return Err(Error::Credentials);
        }
        Ok(Self(value.into()))
    }
}
fn valid_token(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
pub(crate) fn random_bytes() -> Result<[u8; 32]> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Credentials)?;
    Ok(bytes)
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub(crate) fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    digest: [u8; 32],
    kind: PrincipalKind,
    label: Option<String>,
}
#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Registry {
    entries: Vec<Entry>,
}
#[derive(Clone)]
pub struct Credentials {
    path: PathBuf,
}
impl Credentials {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let this = Self {
            path: path.as_ref().to_owned(),
        };
        this.read()?;
        Ok(this)
    }
    fn read(&self) -> Result<Registry> {
        let registry: Registry =
            serde_json::from_slice(&secure_read(&self.path)?).map_err(|_| Error::Credentials)?;
        if registry.entries.is_empty() || registry.entries.len() > 1000 {
            return Err(Error::Credentials);
        }
        let mut seen = std::collections::HashSet::new();
        if registry
            .entries
            .iter()
            .any(|e| !seen.insert(e.digest) || e.label.as_ref().is_some_and(|s| s.len() > 200))
        {
            return Err(Error::Credentials);
        }
        Ok(registry)
    }
    pub fn authenticate(&self, bearer: &str) -> Result<Principal> {
        let registry = self.read()?;
        if !valid_token(bearer) {
            return Err(Error::Unauthenticated);
        }
        let digest: [u8; 32] = Sha256::digest(bearer.as_bytes()).into();
        let mut matched = None;
        for entry in registry.entries {
            if bool::from(entry.digest.ct_eq(&digest)) {
                matched = Some(Principal {
                    kind: entry.kind,
                    label: entry.label,
                    session_id: None,
                    credential: Some(digest),
                });
            }
        }
        matched.ok_or(Error::Unauthenticated)
    }
    pub fn authenticate_file(&self, path: impl AsRef<Path>) -> Result<Principal> {
        self.authenticate(Token::from_file(path)?.expose())
    }
    /// Offline operator provisioning. Never prints or returns the generated token.
    pub fn issue(
        registry: impl AsRef<Path>,
        token_file: impl AsRef<Path>,
        kind: PrincipalKind,
        label: Option<String>,
    ) -> Result<()> {
        let registry = registry.as_ref();
        let token_file = token_file.as_ref();
        if registry == token_file || label.as_ref().is_some_and(|s| s.len() > 200) {
            return Err(Error::Credentials);
        }
        // Serialize provisioning independently of the live DB owner.
        let _lock =
            crate::store::conn::Ownership::acquire(registry).map_err(|_| Error::Credentials)?;
        let mut data = if registry.symlink_metadata().is_ok() {
            Self::open(registry)?.read()?
        } else {
            Registry::default()
        };
        if data.entries.len() >= 1000 {
            return Err(Error::Credentials);
        }
        let token = Token(hex(&random_bytes()?));
        data.entries.push(Entry {
            digest: Sha256::digest(token.expose().as_bytes()).into(),
            kind,
            label,
        });
        let bytes = serde_json::to_vec(&data).map_err(|_| Error::Credentials)?;
        // A failed registry update leaves at most an unusable private token file.
        secure_create(token_file, token.expose().as_bytes())?;
        let temp = crate::store::conn::suffix(registry, &format!(".tmp.{}", ulid::Ulid::new()));
        secure_create(&temp, &bytes)?;
        if let Err(_e) = fs::rename(&temp, registry) {
            let _ = fs::remove_file(&temp);
            return Err(Error::Credentials);
        }
        if let Some(parent) = registry.parent().filter(|p| !p.as_os_str().is_empty()) {
            File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|_| Error::Credentials)?;
        }
        Ok(())
    }
}
#[cfg(unix)]
fn checked_file(path: &Path) -> Result<File> {
    use std::os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    };
    // Refuse symlinks in every component, not only the final entry.
    // Ancestor replacement by the same OS owner remains outside the token boundary.
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part.as_os_str());
        if fs::symlink_metadata(&current)
            .map_err(|_| Error::Credentials)?
            .file_type()
            .is_symlink()
        {
            return Err(Error::Credentials);
        }
    }
    if path.as_os_str().as_bytes().is_empty() {
        return Err(Error::Credentials);
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Error::Credentials)?;
    let meta = file.metadata().map_err(|_| Error::Credentials)?;
    // SAFETY: geteuid takes no arguments and has no preconditions.
    let uid = unsafe { libc::geteuid() };
    if !meta.is_file()
        || meta.mode() & 0o7777 != 0o600
        || meta.uid() != uid
        || meta.nlink() != 1
        || meta.len() > 1024 * 1024
    {
        return Err(Error::Credentials);
    }
    Ok(file)
}
#[cfg(not(unix))]
fn checked_file(_path: &Path) -> Result<File> {
    Err(Error::Credentials)
}
pub(crate) fn secure_read(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    checked_file(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Credentials)?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::Credentials);
    }
    Ok(bytes)
}
#[cfg(unix)]
fn secure_create(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| Error::Credentials)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Error::Credentials)?;
    checked_file(path)?;
    Ok(())
}
#[cfg(not(unix))]
fn secure_create(_path: &Path, _bytes: &[u8]) -> Result<()> {
    Err(Error::Credentials)
}
