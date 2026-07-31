//! Persistence for trust-on-first-use certificate pins.
//!
//! Bambu printers serve a self-signed certificate, so the only defence against
//! an impostor is to remember the fingerprint seen on first contact and require
//! it to match afterwards. This is where those fingerprints live between runs.
//!
//! # No dependency on `pandaspy-client`, on purpose
//!
//! The fingerprint here is a plain `[u8; 32]` (a SHA-256 leaf hash), not
//! `pandaspy_client::CertificateFingerprint`. This crate must not depend on
//! the client. The adapter that wraps this store in the client's `PinStore`
//! trait — turning these bytes into `CertificateFingerprint` — lives in
//! `src-tauri`, which is allowed to know about both crates.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::atomic::write_atomically;
use crate::error::StoreError;

/// Serial → SHA-256 fingerprint of the printer's leaf certificate.
type Pins = BTreeMap<String, [u8; 32]>;

/// A JSON file mapping printer serials to their pinned certificate fingerprint.
#[derive(Debug, Clone)]
pub struct CertPinStore {
    path: PathBuf,
}

impl CertPinStore {
    /// Store pins at `path`. The file and its parent directory are created on
    /// the first [`pin`](Self::pin).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The file this store reads and writes.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The fingerprint pinned for `serial`, if any.
    ///
    /// A corrupt or unreadable pin file is treated as "nothing pinned" rather
    /// than propagated, because the signature is infallible and, under TOFU,
    /// "no pin" is the *safe* direction: it forces the certificate to be
    /// re-verified (surfaced to the user) instead of trusting stale bytes. A
    /// subsequent [`pin`](Self::pin) — which does report errors — rewrites the
    /// file whole and heals it.
    #[must_use]
    pub fn pinned(&self, serial: &str) -> Option<[u8; 32]> {
        self.load().ok()?.get(serial).copied()
    }

    /// Record (or replace) the fingerprint for `serial`.
    pub fn pin(&self, serial: &str, fingerprint: [u8; 32]) -> Result<(), StoreError> {
        let mut pins = self.load()?;
        pins.insert(serial.to_owned(), fingerprint);
        self.save(&pins)
    }

    /// Forget a printer's pin. Idempotent: forgetting an unknown serial is
    /// success and does not rewrite the file.
    pub fn forget(&self, serial: &str) -> Result<(), StoreError> {
        let mut pins = self.load()?;
        if pins.remove(serial).is_some() {
            self.save(&pins)?;
        }
        Ok(())
    }

    fn load(&self) -> Result<Pins, StoreError> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Pins::new()),
            Err(e) => {
                return Err(StoreError::Io(format!("read {}: {e}", self.path.display())));
            }
        };
        serde_json::from_slice(&bytes).map_err(|e| StoreError::Corrupt {
            path: self.path.display().to_string(),
            reason: e.to_string(),
        })
    }

    fn save(&self, pins: &Pins) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec_pretty(pins)
            .map_err(|e| StoreError::Io(format!("serialise pins: {e}")))?;
        write_atomically(&self.path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn fingerprint(seed: u8) -> [u8; 32] {
        std::array::from_fn(|i| seed ^ (i as u8))
    }

    #[test]
    fn pin_pinned_forget_round_trip() {
        let dir = TempDir::new();
        let store = CertPinStore::new(dir.join("pins.json"));
        let fp = fingerprint(0xAB);

        assert_eq!(store.pinned("serial"), None);

        store.pin("serial", fp).unwrap();
        assert_eq!(store.pinned("serial"), Some(fp));

        store.forget("serial").unwrap();
        assert_eq!(store.pinned("serial"), None);
        // Idempotent.
        store.forget("serial").unwrap();
    }

    #[test]
    fn an_unknown_serial_has_no_pin() {
        let dir = TempDir::new();
        let store = CertPinStore::new(dir.join("pins.json"));
        store.pin("known", fingerprint(1)).unwrap();

        assert_eq!(store.pinned("unknown"), None);
    }

    #[test]
    fn pins_persist_across_reopen() {
        let dir = TempDir::new();
        let path = dir.join("nested/pins.json");
        let fp = fingerprint(0x42);

        CertPinStore::new(&path).pin("serial", fp).unwrap();

        // A fresh store over the same path sees the persisted pin.
        assert_eq!(CertPinStore::new(&path).pinned("serial"), Some(fp));
    }

    #[test]
    fn replacing_a_pin_overwrites_it() {
        let dir = TempDir::new();
        let store = CertPinStore::new(dir.join("pins.json"));

        store.pin("serial", fingerprint(1)).unwrap();
        store.pin("serial", fingerprint(2)).unwrap();

        assert_eq!(store.pinned("serial"), Some(fingerprint(2)));
    }
}
