//! Where Spool keeps things between runs.
//!
//! Two separate concerns, deliberately not merged:
//!
//! * **Config** — printer list, poll intervals, notification preferences.
//!   Boring, inspectable, safe to sync or copy between machines.
//! * **Secrets** — printer access codes. These belong in the OS secret store,
//!   never in the config file.
//!
//! # Why this crate may use `#[cfg(target_os)]`
//!
//! Secret storage is one of the few places where the platforms genuinely differ
//! rather than merely appearing to. macOS has Keychain, Windows has Credential
//! Manager, Linux has Secret Service (when a session bus exists — and on a
//! headless box it does not, hence the encrypted-file fallback).
//!
//! The allowance is scoped to backend *selection*. Everything above the
//! [`SecretStore`] trait is platform-agnostic, and nothing outside this crate
//! and `src-tauri` may branch on the target OS at all. See `CLAUDE.md`
//! § The one architectural rule.

mod config;
mod error;
mod secrets;

pub use config::{Config, ConfigStore, PrinterEntry};
pub use error::StoreError;
pub use secrets::{SecretBackend, SecretStore, default_backend, os_keyring_name};
