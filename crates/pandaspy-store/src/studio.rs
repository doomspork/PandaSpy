//! Read-only import of printers Bambu Studio already knows about.
//!
//! Studio keeps its LAN credentials in `BambuStudio.conf`, a JSON document in
//! its own config directory. Two maps keyed by device serial matter here:
//!
//! * `access_code` — every printer Studio has connected to over the LAN.
//! * `ip_address`  — only *some* of them. Studio records an address for
//!   printers added by IP, and finds the rest at runtime over SSDP, so a
//!   missing address is normal, not corruption. The caller fills it in from
//!   discovery.
//!
//! Studio has no name for a printer in this file; names come from SSDP too.
//!
//! The parse is lenient in the same spirit as `pandaspy-proto`: this is someone
//! else's file format, and it changes with their releases. A value of the wrong
//! type is skipped, never an error. Only "this is not JSON at all" fails.
//!
//! Locating the file is the caller's job — it lives in the platform config
//! directory (`~/Library/Application Support`, `%APPDATA%`, `~/.config`), and
//! `src-tauri` owns those conventions.

use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;
use std::path::Path;

use pandaspy_proto::DeviceSerial;
use serde_json::Value;

use crate::StoreError;

/// One printer as Bambu Studio has it configured.
#[derive(Clone, PartialEq, Eq)]
pub struct StudioPrinter {
    pub serial: DeviceSerial,
    /// The LAN access code, when Studio has one.
    pub access_code: Option<String>,
    /// The last address Studio recorded, when it recorded one.
    pub address: Option<IpAddr>,
}

// Hand-written so the access code cannot leak into a log via `{:?}`.
impl fmt::Debug for StudioPrinter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StudioPrinter")
            .field("serial", &self.serial)
            .field(
                "access_code",
                &self.access_code.as_ref().map(|_| "<redacted>"),
            )
            .field("address", &self.address)
            .finish()
    }
}

/// Parse the contents of `BambuStudio.conf`, returning printers sorted by serial.
///
/// Only the first JSON value in the text is read: some Studio releases append a
/// trailing comment line after the document, and anything there is not ours.
///
/// # Errors
///
/// [`StoreError::Corrupt`] if the text does not start with a JSON object.
pub fn parse_studio_config(text: &str) -> Result<Vec<StudioPrinter>, StoreError> {
    let corrupt = |reason: String| StoreError::Corrupt {
        path: "BambuStudio.conf".to_owned(),
        reason,
    };
    let document = serde_json::Deserializer::from_str(text)
        .into_iter::<Value>()
        .next()
        .ok_or_else(|| corrupt("empty file".to_owned()))?
        .map_err(|e| corrupt(e.to_string()))?;
    let Value::Object(root) = document else {
        return Err(corrupt("not a JSON object".to_owned()));
    };

    fn entry<'a>(
        printers: &'a mut BTreeMap<String, StudioPrinter>,
        serial: &str,
    ) -> &'a mut StudioPrinter {
        printers
            .entry(serial.to_owned())
            .or_insert_with(|| StudioPrinter {
                serial: DeviceSerial(serial.to_owned()),
                access_code: None,
                address: None,
            })
    }

    let mut printers = BTreeMap::new();
    for (serial, code) in string_map(&root, "access_code") {
        entry(&mut printers, serial).access_code = Some(code.to_owned());
    }
    for (serial, address) in string_map(&root, "ip_address") {
        if let Ok(address) = address.parse::<IpAddr>() {
            entry(&mut printers, serial).address = Some(address);
        }
    }

    Ok(printers.into_values().collect())
}

/// Read and parse `BambuStudio.conf` at `path`.
///
/// A missing file is not an error — it means Studio is not installed (or has
/// never been run), which is an empty import, not a failure.
///
/// # Errors
///
/// [`StoreError::Io`] if the file exists but cannot be read, and
/// [`StoreError::Corrupt`] if it is not JSON.
pub fn read_studio_config(path: &Path) -> Result<Vec<StudioPrinter>, StoreError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_studio_config(&text).map_err(|error| match error {
            StoreError::Corrupt { reason, .. } => StoreError::Corrupt {
                path: path.display().to_string(),
                reason,
            },
            other => other,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(StoreError::Io(format!("{}: {error}", path.display()))),
    }
}

/// The non-empty string entries of the object at `root[key]`, skipping anything
/// that is not a string.
fn string_map<'a>(
    root: &'a serde_json::Map<String, Value>,
    key: &str,
) -> impl Iterator<Item = (&'a str, &'a str)> {
    root.get(key)
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(serial, value)| {
            let value = value.as_str()?.trim();
            let serial = serial.trim();
            (!serial.is_empty() && !value.is_empty()).then_some((serial, value))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_printer_with_only_an_address_is_still_listed() {
        let printers = parse_studio_config(r#"{"ip_address":{"S1":"10.0.0.5"}}"#).unwrap();
        assert_eq!(printers.len(), 1);
        assert_eq!(printers[0].access_code, None);
        assert_eq!(printers[0].address, "10.0.0.5".parse().ok());
    }

    #[test]
    fn wrong_types_and_bad_addresses_are_skipped_not_fatal() {
        let printers = parse_studio_config(
            r#"{"access_code":{"S1":42,"S2":"","S3":"12345678"},
                "ip_address":{"S3":"not-an-ip"},
                "app":"unexpected"}"#,
        )
        .unwrap();
        assert_eq!(printers.len(), 1);
        assert_eq!(printers[0].serial.0, "S3");
        assert_eq!(printers[0].address, None);
    }

    #[test]
    fn trailing_content_after_the_document_is_ignored() {
        let printers =
            parse_studio_config("{\"access_code\":{\"S1\":\"12345678\"}}\n# trailer 0123\n")
                .unwrap();
        assert_eq!(printers.len(), 1);
    }

    #[test]
    fn a_config_with_no_printers_is_empty_not_an_error() {
        assert!(parse_studio_config(r#"{"app":{}}"#).unwrap().is_empty());
    }

    #[test]
    fn non_json_is_corrupt() {
        assert!(matches!(
            parse_studio_config("[app]\nfoo=bar"),
            Err(StoreError::Corrupt { .. })
        ));
    }

    #[test]
    fn debug_never_prints_the_access_code() {
        let printers = parse_studio_config(r#"{"access_code":{"S1":"12345678"}}"#).unwrap();
        let debug = format!("{printers:?}");
        assert!(!debug.contains("12345678"), "{debug}");
    }

    #[test]
    fn a_missing_file_is_an_empty_import() {
        let path = std::env::temp_dir().join("pandaspy-no-such-studio-dir/BambuStudio.conf");
        assert!(read_studio_config(&path).unwrap().is_empty());
    }
}
