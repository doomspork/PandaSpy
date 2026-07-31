use std::io;
use std::path::{Path, PathBuf};

use pandaspy_proto::DeviceSerial;
use serde::{Deserialize, Serialize};

use crate::atomic::write_atomically;
use crate::error::StoreError;

/// One printer the user has told PandaSpy about.
///
/// Note what is *not* here: the access code. Config is plain text on disk;
/// secrets go through [`crate::SecretStore`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrinterEntry {
    pub serial: Option<DeviceSerial>,
    /// Last known address. A hint for reconnecting, not a source of truth —
    /// DHCP moves printers around.
    pub last_address: Option<String>,
    /// User-chosen name, falling back to the model in the UI when absent.
    pub nickname: Option<String>,
}

/// Everything PandaSpy remembers that is not a secret.
///
/// Forward compatible by construction: `#[serde(default)]` and optional fields
/// mean a config written by a newer build still loads in an older one. Users
/// downgrade, and losing their printer list when they do is unacceptable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub printers: Vec<PrinterEntry>,
    /// `None` means "follow the operating system".
    pub locale: Option<String>,
    pub launch_at_login: Option<bool>,
}

/// Load and save [`Config`].
///
/// A trait rather than free functions so tests can run against an in-memory
/// store and so the on-disk format can change without every caller knowing.
pub trait ConfigStore: Send + Sync + std::fmt::Debug {
    /// Read the stored config.
    ///
    /// A missing file is not an error — it is [`Config::default`]. A *corrupt*
    /// file is an error, and must not be silently replaced.
    fn load(&self) -> Result<Config, StoreError>;

    /// Persist the config, atomically enough that a crash mid-write cannot
    /// leave a truncated file behind.
    fn save(&self, config: &Config) -> Result<(), StoreError>;
}

/// A [`ConfigStore`] backed by a JSON file.
///
/// The path is *injected*, not discovered here: `src-tauri` resolves the Tauri
/// app-config directory and hands it in, which keeps this crate testable with a
/// throwaway temp file and no dependency on a platform's notion of "config
/// dir".
#[derive(Debug, Clone)]
pub struct FileConfigStore {
    path: PathBuf,
}

impl FileConfigStore {
    /// Store the config at `path`. The file need not exist yet; its parent
    /// directory is created on the first [`save`](ConfigStore::save).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The file this store reads and writes.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ConfigStore for FileConfigStore {
    fn load(&self) -> Result<Config, StoreError> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            // A first run has no file; that is the default config, not a fault.
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => {
                return Err(StoreError::Io(format!("read {}: {e}", self.path.display())));
            }
        };

        // A file that exists but does not parse is surfaced, never discarded:
        // it may be a newer format, a partial restore, or a bug, and any of
        // those beats silently resetting the user's printer list.
        serde_json::from_slice(&bytes).map_err(|e| StoreError::Corrupt {
            path: self.path.display().to_string(),
            reason: e.to_string(),
        })
    }

    fn save(&self, config: &Config) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec_pretty(config)
            .map_err(|e| StoreError::Io(format!("serialise config: {e}")))?;
        write_atomically(&self.path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn a_config_from_a_newer_build_still_loads() {
        // Old build, new file: unknown keys are ignored rather than fatal.
        let json = r#"{"printers":[],"locale":"pl-PL","telemetry_opt_in":true}"#;
        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(config.locale.as_deref(), Some("pl-PL"));
    }

    #[test]
    fn an_empty_config_is_the_default_not_a_failure() {
        let config: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn printer_entries_never_carry_the_access_code() {
        // Guards the invariant by construction: if someone adds an
        // `access_code` field to `PrinterEntry`, this serialisation changes and
        // the test fails.
        let entry = PrinterEntry {
            serial: Some(DeviceSerial("00M09A000000000".to_owned())),
            ..PrinterEntry::default()
        };

        let json = serde_json::to_string(&entry).unwrap();

        assert!(
            !json.contains("access"),
            "secret leaked into config: {json}"
        );
    }

    #[test]
    fn a_missing_file_loads_the_default_config() {
        let dir = TempDir::new();
        let store = FileConfigStore::new(dir.join("does-not-exist.json"));

        assert_eq!(store.load().unwrap(), Config::default());
    }

    #[test]
    fn a_corrupt_file_is_reported_not_silently_discarded() {
        let dir = TempDir::new();
        let path = dir.join("config.json");
        std::fs::write(&path, b"{ this is not json").unwrap();
        let store = FileConfigStore::new(&path);

        let err = store.load().unwrap_err();
        assert!(
            matches!(err, StoreError::Corrupt { .. }),
            "expected Corrupt, got {err:?}"
        );
        // The original bytes are still on disk: nothing overwrote them.
        assert_eq!(std::fs::read(&path).unwrap(), b"{ this is not json");
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new();
        let store = FileConfigStore::new(dir.join("nested/config.json"));

        let config = Config {
            printers: vec![PrinterEntry {
                serial: Some(DeviceSerial("00M09A000000000".to_owned())),
                last_address: Some("192.0.2.10".to_owned()),
                nickname: Some("Workshop".to_owned()),
            }],
            locale: Some("en-US".to_owned()),
            launch_at_login: Some(true),
        };

        store.save(&config).unwrap();
        assert_eq!(store.load().unwrap(), config);
    }

    #[test]
    fn a_save_leaves_no_temp_file_behind() {
        let dir = TempDir::new();
        let store = FileConfigStore::new(dir.join("config.json"));
        store.save(&Config::default()).unwrap();

        let mut entries: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        entries.sort();

        // Exactly the target — the temp file was renamed, not orphaned.
        assert_eq!(entries, vec!["config.json".to_owned()]);
    }
}
