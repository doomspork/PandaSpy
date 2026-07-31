use crate::error::StoreError;

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

// TODO(scaffold): two implementations.
//   * `KeyringSecrets`  — wraps the `keyring` crate.
//   * `EncryptedFileSecrets` — fallback; needs a key-derivation decision that
//     should be made deliberately and written down, not improvised.

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
}
