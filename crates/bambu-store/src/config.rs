use bambu_proto::DeviceSerial;
use serde::{Deserialize, Serialize};

use crate::error::StoreError;

/// One printer the user has told Spool about.
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

/// Everything Spool remembers that is not a secret.
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

// TODO(scaffold): the on-disk implementation. Write to a temp file in the same
// directory and rename over the target — that is the portable atomic-write
// idiom and it matters here, because the alternative is a user losing their
// printer list to a power cut.

#[cfg(test)]
mod tests {
    use super::*;

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
}
