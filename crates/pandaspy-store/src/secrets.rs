use keyring::{Entry, Error as KeyringError};

use crate::error::StoreError;

/// The keyring service (a.k.a. "server") every access code is filed under. The
/// keyring "account" is the printer serial, so one printer is one credential.
///
/// This is the bundle identifier on purpose: it namespaces PandaSpy's items in
/// a shared OS store and it is what a user sees next to the entry in Keychain
/// Access or Credential Manager.
pub const KEYRING_SERVICE: &str = "io.github.seancallan.pandaspy";

/// A serial that is never a real printer, used only to ask the keyring whether
/// it will answer at all. See [`keyring_available`].
const KEYRING_PROBE_ACCOUNT: &str = "__pandaspy_keyring_probe__";

/// Which secret backend is in use.
///
/// Surfaced to the UI because the honest answer to "where is my access code
/// stored?" differs per platform, and users on the encrypted-file fallback
/// deserve to know they are on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SecretBackend {
    /// macOS Keychain, Windows Credential Manager, or Linux Secret Service.
    OsKeyring,
    /// A locally encrypted file. Used where no OS keyring is reachable —
    /// typically a headless or minimal Linux session with no D-Bus.
    EncryptedFile,
}

/// Read and write printer access codes.
///
/// Keys are printer serials. Values are short strings the user copied off the
/// printer's screen.
///
/// Synchronous because every backend is a fast local call, and because forcing
/// an async runtime on secret access would push `tokio` into crates that have
/// no other reason to want it.
pub trait SecretStore: Send + Sync + std::fmt::Debug {
    /// Which backend this instance is actually using.
    fn backend(&self) -> SecretBackend;

    /// Fetch the access code for a printer.
    fn access_code(&self, serial: &str) -> Result<String, StoreError>;

    /// Store (or replace) the access code for a printer.
    fn set_access_code(&self, serial: &str, code: &str) -> Result<(), StoreError>;

    /// Forget a printer's access code. Removing a printer must remove its
    /// secret too, or the keychain slowly fills with orphans.
    fn forget(&self, serial: &str) -> Result<(), StoreError>;
}

/// [`SecretStore`] backed by the OS keyring via the `keyring` crate.
///
/// Which concrete store that is — Keychain, Credential Manager, Secret Service
/// — is chosen by the per-target `keyring` features in `Cargo.toml`, not by any
/// branch here. This type is identical on every platform; the difference lives
/// entirely below the trait, which is exactly what the architectural rule asks
/// for.
#[derive(Debug, Clone)]
pub struct KeyringSecrets {
    service: String,
}

impl KeyringSecrets {
    /// Use the default PandaSpy keyring service ([`KEYRING_SERVICE`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            service: KEYRING_SERVICE.to_owned(),
        }
    }

    /// Use a custom service name. Exists so tests can namespace themselves away
    /// from a developer's real PandaSpy entries.
    #[must_use]
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, serial: &str) -> Result<Entry, StoreError> {
        Entry::new(&self.service, serial).map_err(map_backend_error)
    }
}

impl Default for KeyringSecrets {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringSecrets {
    fn backend(&self) -> SecretBackend {
        SecretBackend::OsKeyring
    }

    fn access_code(&self, serial: &str) -> Result<String, StoreError> {
        self.entry(serial)?.get_password().map_err(|e| match e {
            KeyringError::NoEntry => StoreError::SecretMissing(serial.to_owned()),
            other => map_backend_error(other),
        })
    }

    fn set_access_code(&self, serial: &str, code: &str) -> Result<(), StoreError> {
        self.entry(serial)?
            .set_password(code)
            .map_err(map_backend_error)
    }

    fn forget(&self, serial: &str) -> Result<(), StoreError> {
        match self.entry(serial)?.delete_credential() {
            // Idempotent: forgetting an already-absent secret is success, so a
            // half-removed printer can always be finished off.
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(other) => Err(map_backend_error(other)),
        }
    }
}

/// Distinguish "the backend is unreachable" from "the secret is not there".
///
/// The two demand opposite responses — fall back to the encrypted file vs. ask
/// the user for the access code — so `NoEntry` is mapped by the caller and only
/// the genuine backend failures land here.
fn map_backend_error(error: KeyringError) -> StoreError {
    StoreError::SecretBackend(error.to_string())
}

/// Whether the OS keyring will actually answer, decided at runtime.
///
/// `src-tauri` calls this to choose [`KeyringSecrets`] over
/// [`crate::EncryptedFileSecrets`] on a host with no reachable keyring (a
/// headless Linux box with no Secret Service). It must never be a compile-time
/// `cfg`: whether D-Bus is up is a property of the running session, not of the
/// target the binary was built for.
///
/// The probe reads a sentinel account that is never written. A working backend
/// answers `NoEntry`; an unreachable one answers a platform/storage error. A
/// read is non-destructive, so calling this leaves the store untouched.
#[must_use]
pub fn keyring_available() -> bool {
    let Ok(entry) = Entry::new(KEYRING_SERVICE, KEYRING_PROBE_ACCOUNT) else {
        return false;
    };
    match entry.get_password() {
        // Reachable and empty (expected), or somehow reachable and populated.
        Ok(_) | Err(KeyringError::NoEntry) => true,
        // PlatformFailure / NoStorageAccess / anything else: not usable.
        Err(_) => false,
    }
}

/// The backend to try before any runtime probing.
///
/// Always the OS keyring. [`SecretBackend::EncryptedFile`] is a fallback
/// *discovered at runtime* when the keyring turns out to be unreachable — it is
/// never a platform's a-priori choice, which is why this function needs no
/// `cfg` at all.
#[must_use]
pub fn default_backend() -> SecretBackend {
    SecretBackend::OsKeyring
}

/// What the platform calls its keyring, for use in UI and error messages.
///
/// # This is the `#[cfg(target_os)]` allowance in action
///
/// Three operating systems, three different product names, and no amount of
/// abstraction makes "Keychain" and "Credential Manager" the same word. This is
/// the shape the allowance exists for: a leaf function, a genuine platform
/// difference, and no logic branching on the answer.
///
/// These are proper nouns, so they are *not* Fluent keys — translating
/// "Keychain" would make the message harder to act on, not easier.
#[must_use]
pub fn os_keyring_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Keychain"
    }
    #[cfg(target_os = "windows")]
    {
        "Credential Manager"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "Secret Service"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_platform_prefers_the_os_keyring_first() {
        assert_eq!(default_backend(), SecretBackend::OsKeyring);
    }

    #[test]
    fn the_keyring_has_a_name_on_every_platform() {
        // Whichever branch this build took, it must have taken one.
        assert!(!os_keyring_name().is_empty());

        #[cfg(target_os = "macos")]
        assert_eq!(os_keyring_name(), "Keychain");
    }

    #[test]
    fn keyring_secrets_reports_the_os_backend() {
        assert_eq!(KeyringSecrets::new().backend(), SecretBackend::OsKeyring);
    }

    #[test]
    fn probing_the_keyring_never_panics() {
        // Non-destructive and CI-safe: it only *reads* a sentinel account that
        // is never written, so on a runner with no Secret Service it returns
        // `false` rather than failing. The value itself is environment-
        // dependent, so there is nothing to assert but that the call returns.
        let _ = keyring_available();
    }

    #[test]
    #[ignore = "touches the real OS keyring; run with `--ignored` on a machine \
                that has a session keychain/credential manager"]
    fn keyring_round_trips_a_secret() {
        // Skip rather than fail where no backend is reachable (e.g. CI), so the
        // test is harmless even if someone drops the `#[ignore]`.
        if !keyring_available() {
            return;
        }

        let store = KeyringSecrets::with_service("io.github.seancallan.pandaspy.test");
        let serial = "00M00TEST0000000";

        store.set_access_code(serial, "12345678").unwrap();
        assert_eq!(store.access_code(serial).unwrap(), "12345678");

        store.forget(serial).unwrap();
        assert!(matches!(
            store.access_code(serial),
            Err(StoreError::SecretMissing(_))
        ));
        // Idempotent: forgetting again is still success.
        store.forget(serial).unwrap();
    }
}
