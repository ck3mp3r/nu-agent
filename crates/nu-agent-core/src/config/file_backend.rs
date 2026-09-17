//! Plaintext-file backend for the [`Vault`](super::vault::Vault).
//!
//! Stores `key -> value` as JSON at `$XDG_DATA_HOME/nu-agent/secrets.json`
//! with 0600 permissions and `fs2` file locking. The Vault serializes its
//! typed values, so this backend only ever sees strings.

// region:    --- Modules

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use fs2::FileExt;

use super::vault::{VaultBackend, VaultError};

// endregion: --- Modules

// region:    --- Constants

/// Subdirectory of the XDG data directory holding the secrets file.
const DATA_SUBDIR: &str = "nu-agent";

/// Secrets file name inside [`DATA_SUBDIR`].
const SECRETS_FILE: &str = "secrets.json";

// endregion: --- Constants

// region:    --- Types

/// Plaintext-file backend.
///
/// Used on headless Linux without a D-Bus Secret Service, or in containers
/// with a volume-mounted data directory.
#[derive(Debug)]
pub struct FileBackend {
    path: PathBuf,
}

// endregion: --- Types

// region:    --- FileBackend Impl

impl FileBackend {
    // --- Constructors ---

    /// Construct a backend reading and writing `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Probe whether the XDG data directory is writable.
    ///
    /// Creates the `nu-agent/` subdirectory when missing, then proves
    /// writability by creating and dropping a probe file inside it.
    pub fn probe() -> Result<Self, VaultError> {
        let path = Self::default_path()?;
        let dir = path.parent().ok_or(VaultError::NoDataDir)?;
        std::fs::create_dir_all(dir).map_err(|_| VaultError::NoDataDir)?;
        tempfile::NamedTempFile::new_in(dir).map_err(|_| VaultError::NoDataDir)?;
        Ok(Self::new(path))
    }

    // --- Accessors ---

    /// The file this backend reads and writes.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Resolve `$XDG_DATA_HOME/nu-agent/secrets.json`.
    pub fn default_path() -> Result<PathBuf, VaultError> {
        let dir = crate::utils::xdg::data_dir().map_err(|_| VaultError::NoDataDir)?;
        Ok(dir.join(DATA_SUBDIR).join(SECRETS_FILE))
    }

    // --- Support ---

    /// Read the map from an already-open handle. A missing or empty file reads
    /// as empty.
    fn read_map_from(file: &mut std::fs::File) -> Result<HashMap<String, String>, VaultError> {
        let mut raw = String::new();
        file.read_to_string(&mut raw)?;
        if raw.trim().is_empty() {
            return Ok(HashMap::new());
        }
        Ok(serde_json::from_str(&raw)?)
    }

    /// Open the secrets file for update, creating it when absent.
    ///
    /// The file is created 0600 and tightened to 0600 when it already existed
    /// with looser permissions.
    fn open_for_update(&self) -> Result<std::fs::File, VaultError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(&self.path)?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            Ok(file)
        }

        #[cfg(not(unix))]
        {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&self.path)?;
            Ok(file)
        }
    }

    /// Apply `mutate` to the stored map under an exclusive lock.
    ///
    /// The lock spans the whole read-modify-write cycle, so concurrent writers
    /// cannot lose each other's updates. The lock is released when `file` drops.
    fn update(&self, mutate: impl FnOnce(&mut HashMap<String, String>)) -> Result<(), VaultError> {
        let mut file = self.open_for_update()?;
        file.lock_exclusive()?;

        let mut map = Self::read_map_from(&mut file)?;
        mutate(&mut map);

        let content = serde_json::to_string_pretty(&map)?;
        file.seek(SeekFrom::Start(0))?;
        file.set_len(0)?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
        file.unlock()?;
        Ok(())
    }

    /// Read the map. A missing file reads as empty.
    fn load_map(&self) -> Result<HashMap<String, String>, VaultError> {
        match std::fs::read_to_string(&self.path) {
            Ok(raw) if raw.trim().is_empty() => Ok(HashMap::new()),
            Ok(raw) => Ok(serde_json::from_str(&raw)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(e) => Err(VaultError::Io(e)),
        }
    }
}

// endregion: --- FileBackend Impl

// region:    --- VaultBackend Impl

impl VaultBackend for FileBackend {
    fn get(&self, key: &str) -> Result<Option<String>, VaultError> {
        let map = self.load_map()?;
        Ok(map.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), VaultError> {
        self.update(|map| {
            map.insert(key.to_string(), value.to_string());
        })
    }

    fn delete(&self, key: &str) -> Result<(), VaultError> {
        // Deleting an absent key is not an error.
        self.update(|map| {
            map.remove(key);
        })
    }

    fn list(&self) -> Result<Vec<String>, VaultError> {
        let map = self.load_map()?;
        Ok(map.keys().cloned().collect())
    }
}

// endregion: --- VaultBackend Impl

// region:    --- Tests

#[cfg(test)]
#[path = "file_backend_test.rs"]
mod file_backend_test;

// endregion: --- Tests
