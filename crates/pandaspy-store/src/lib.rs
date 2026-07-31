//! Where PandaSpy keeps things between runs.
//!
//! Three separate concerns, deliberately not merged:
//!
//! * **Config** — printer list, poll intervals, notification preferences.
//!   Boring, inspectable, safe to sync or copy between machines.
//!   ([`FileConfigStore`])
//! * **Secrets** — printer access codes. These belong in the OS secret store
//!   ([`KeyringSecrets`]), or an encrypted file where none is reachable
//!   ([`EncryptedFileSecrets`]).
//! * **Pins** — certificate fingerprints for trust-on-first-use.
//!   ([`CertPinStore`])
//!
//! Every persistent store takes its path by injection so `src-tauri` owns the
//! platform's directory conventions and this crate stays testable without one.
//!
//! # Why this crate may use `#[cfg(target_os)]`
//!
//! Secret storage is one of the few places where the platforms genuinely differ
//! rather than merely appearing to. macOS has Keychain, Windows has Credential
//! Manager, Linux has Secret Service (when a session bus exists — and on a
//! headless box it does not, hence the encrypted-file fallback).
//!
//! The allowance is scoped to backend *selection* (the per-target `keyring`
//! features in `Cargo.toml`) and platform *naming* ([`os_keyring_name`]).
//! Everything above the [`SecretStore`] trait is platform-agnostic — the choice
//! between backends is made at runtime by [`keyring_available`], never by a
//! `cfg`. Nothing outside this crate and `src-tauri` may branch on the target
//! OS at all. See `CLAUDE.md` § The one architectural rule.

mod atomic;
mod base64;
mod config;
mod encrypted;
mod error;
mod pins;
mod secrets;

#[cfg(test)]
mod testutil;

pub use config::{Config, ConfigStore, FileConfigStore, PrinterEntry};
pub use encrypted::EncryptedFileSecrets;
pub use error::StoreError;
pub use pins::CertPinStore;
pub use secrets::{
    KEYRING_SERVICE, KeyringSecrets, SecretBackend, SecretStore, default_backend,
    keyring_available, os_keyring_name,
};
