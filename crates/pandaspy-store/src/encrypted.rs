//! An encrypted-file fallback for hosts with no reachable OS keyring.
//!
//! This exists for one situation: a headless or minimal Linux session with no
//! Secret Service (no D-Bus), where [`crate::KeyringSecrets`] simply cannot
//! store anything. Rather than drop access codes into plaintext, PandaSpy
//! derives a key from machine-bound material and keeps the codes in an
//! authenticated-encrypted file.
//!
//! # Threat model — read this before trusting it
//!
//! This is deliberately a *weaker* guarantee than the OS keyring, and the UI
//! must say so when this backend is in use.
//!
//! **What it protects against.** An access code never touches the disk in
//! plaintext. So it resists:
//!
//! * offline inspection of the disk, a backup, a snapshot, or a stolen drive;
//! * another *local user* reading the file — provided the file is `0600`, i.e.
//!   readable only by the owner (see below on whose job that is).
//!
//! **What it does NOT protect against.** An attacker who can execute code *as
//! the same user* has already lost you the game: they can read the very same
//! machine key material this crate derives from, read the file, and derive the
//! key exactly as PandaSpy does. Encryption cannot fix "the attacker is you".
//! It also does not protect against an attacker who can substitute the machine
//! key material.
//!
//! In short: this raises the bar from "grep the disk" to "run code as the
//! target user", and no further. That is worth having on a headless box, and
//! it is not a substitute for the OS keyring where one exists.
//!
//! # Whose job is `0600`
//!
//! This crate does **not** `chmod` the file: doing that portably needs per-OS
//! code, which the architectural rule keeps out of the logic here. The
//! caller (`src-tauri`) owns the config directory and is responsible for
//! creating it with owner-only permissions. The "other local users" guarantee
//! above is conditional on that.
//!
//! # Crypto
//!
//! * KDF: Argon2id (v0x13), 19 MiB / 2 passes / 1 lane, over
//!   `key_material || kdf_salt`, producing a 32-byte key wiped on drop.
//! * AEAD: ChaCha20-Poly1305 with a 12-byte nonce.
//! * A fresh random 16-byte salt and 12-byte nonce are drawn on *every* write,
//!   so the derived key and nonce are unique per file version and nonce reuse
//!   is a non-issue.
//! * The KDF parameters are not stored in the file; they are pinned to the
//!   on-disk `version`. Changing them requires bumping the version and a
//!   migration, because old files were sealed under the old parameters.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::PathBuf;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::atomic::write_atomically;
use crate::base64;
use crate::error::StoreError;
use crate::secrets::{SecretBackend, SecretStore};

/// Bumped only if the on-disk layout or the KDF parameters change.
const FORMAT_VERSION: u32 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

// Argon2id parameters. 19 MiB / 2 passes / 1 lane is the OWASP baseline for
// Argon2id. They live here, not in the file, and are tied to `FORMAT_VERSION`:
// change a number and every existing file stops decrypting, so a change means a
// version bump and a migration.
const ARGON_M_COST_KIB: u32 = 19 * 1024;
const ARGON_T_COST: u32 = 2;
const ARGON_P_COST: u32 = 1;

/// Serial → access code. `BTreeMap` for a deterministic plaintext byte order.
type Secrets = BTreeMap<String, String>;

/// The JSON envelope on disk. Everything except `version` is base64.
#[derive(Serialize, Deserialize)]
struct SealedFile {
    version: u32,
    kdf_salt: String,
    nonce: String,
    ciphertext: String,
}

/// [`SecretStore`] that keeps access codes in an authenticated-encrypted file.
///
/// See the module docs for the threat model. The key material is injected by
/// the caller so the crypto here stays pure and unit-testable; sourcing a
/// machine-bound identifier is `src-tauri`'s job.
pub struct EncryptedFileSecrets {
    key_material: Zeroizing<Vec<u8>>,
    path: PathBuf,
}

impl EncryptedFileSecrets {
    /// Seal secrets at `path`, deriving the key from `key_material`.
    #[must_use]
    pub fn new(key_material: Zeroizing<Vec<u8>>, path: PathBuf) -> Self {
        Self { key_material, path }
    }

    fn corrupt(&self, reason: impl Into<String>) -> StoreError {
        StoreError::Corrupt {
            path: self.path.display().to_string(),
            reason: reason.into(),
        }
    }

    /// Derive the 32-byte file key. Public to the crate so a test can prove the
    /// key is not just the raw material handed in.
    pub(crate) fn derive_key(&self, salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, StoreError> {
        let params = Params::new(ARGON_M_COST_KIB, ARGON_T_COST, ARGON_P_COST, Some(KEY_LEN))
            .map_err(|e| StoreError::SecretBackend(format!("argon2 parameters: {e}")))?;
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        argon
            .hash_password_into(&self.key_material, salt, key.as_mut_slice())
            .map_err(|e| StoreError::SecretBackend(format!("key derivation failed: {e}")))?;
        Ok(key)
    }

    fn load(&self) -> Result<Secrets, StoreError> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            // No file yet means no secrets yet, not a fault.
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Secrets::new()),
            Err(e) => {
                return Err(StoreError::Io(format!("read {}: {e}", self.path.display())));
            }
        };

        let sealed: SealedFile =
            serde_json::from_slice(&bytes).map_err(|e| self.corrupt(e.to_string()))?;
        if sealed.version != FORMAT_VERSION {
            return Err(self.corrupt(format!(
                "unsupported secret file version {}",
                sealed.version
            )));
        }

        let salt = base64::decode(&sealed.kdf_salt)
            .ok_or_else(|| self.corrupt("salt is not valid base64"))?;
        let nonce = base64::decode(&sealed.nonce)
            .ok_or_else(|| self.corrupt("nonce is not valid base64"))?;
        let ciphertext = base64::decode(&sealed.ciphertext)
            .ok_or_else(|| self.corrupt("ciphertext is not valid base64"))?;
        if salt.len() != SALT_LEN {
            return Err(self.corrupt("salt has the wrong length"));
        }
        if nonce.len() != NONCE_LEN {
            return Err(self.corrupt("nonce has the wrong length"));
        }

        let key = self.derive_key(&salt)?;
        let cipher = ChaCha20Poly1305::new_from_slice(key.as_slice()).map_err(|_| {
            StoreError::SecretBackend("derived key has the wrong length".to_owned())
        })?;
        let nonce = Nonce::try_from(nonce.as_slice())
            .map_err(|_| self.corrupt("nonce has the wrong length"))?;

        // A decryption failure here is the AEAD tag rejecting the ciphertext:
        // the file was tampered with, or the key material is not the one that
        // sealed it. Either way it is corrupt as far as this key is concerned,
        // and must surface — never be swallowed into an empty map.
        let plaintext = cipher
            .decrypt(&nonce, ciphertext.as_ref())
            .map(Zeroizing::new)
            .map_err(|_| {
                self.corrupt("failed authentication (tampered file or wrong key material)")
            })?;

        serde_json::from_slice(&plaintext)
            .map_err(|e| self.corrupt(format!("decrypted payload is not valid: {e}")))
    }

    fn save(&self, secrets: &Secrets) -> Result<(), StoreError> {
        let mut salt = [0u8; SALT_LEN];
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut salt)
            .map_err(|e| StoreError::SecretBackend(format!("randomness unavailable: {e}")))?;
        getrandom::fill(&mut nonce)
            .map_err(|e| StoreError::SecretBackend(format!("randomness unavailable: {e}")))?;

        let key = self.derive_key(&salt)?;
        let cipher = ChaCha20Poly1305::new_from_slice(key.as_slice()).map_err(|_| {
            StoreError::SecretBackend("derived key has the wrong length".to_owned())
        })?;
        let nonce = Nonce::try_from(&nonce[..])
            .map_err(|_| StoreError::SecretBackend("nonce has the wrong length".to_owned()))?;

        let plaintext = Zeroizing::new(
            serde_json::to_vec(secrets)
                .map_err(|e| StoreError::SecretBackend(format!("serialise secrets: {e}")))?,
        );
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_slice())
            .map_err(|_| StoreError::SecretBackend("encryption failed".to_owned()))?;

        let sealed = SealedFile {
            version: FORMAT_VERSION,
            kdf_salt: base64::encode(&salt),
            nonce: base64::encode(&nonce),
            ciphertext: base64::encode(&ciphertext),
        };
        let bytes = serde_json::to_vec_pretty(&sealed)
            .map_err(|e| StoreError::SecretBackend(format!("serialise secret file: {e}")))?;
        write_atomically(&self.path, &bytes)
    }
}

impl fmt::Debug for EncryptedFileSecrets {
    /// Never render the key material — a `Debug` that leaks the key defeats the
    /// whole point.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncryptedFileSecrets")
            .field("path", &self.path)
            .field("key_material", &"<redacted>")
            .finish()
    }
}

impl SecretStore for EncryptedFileSecrets {
    fn backend(&self) -> SecretBackend {
        SecretBackend::EncryptedFile
    }

    fn access_code(&self, serial: &str) -> Result<String, StoreError> {
        self.load()?
            .get(serial)
            .cloned()
            .ok_or_else(|| StoreError::SecretMissing(serial.to_owned()))
    }

    fn set_access_code(&self, serial: &str, code: &str) -> Result<(), StoreError> {
        let mut secrets = self.load()?;
        secrets.insert(serial.to_owned(), code.to_owned());
        self.save(&secrets)
    }

    fn forget(&self, serial: &str) -> Result<(), StoreError> {
        let mut secrets = self.load()?;
        // Only rewrite (and re-run the KDF) if something actually changed.
        if secrets.remove(serial).is_some() {
            self.save(&secrets)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn material(bytes: &[u8]) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(bytes.to_vec())
    }

    #[test]
    fn set_get_forget_round_trips() {
        let dir = TempDir::new();
        let store =
            EncryptedFileSecrets::new(material(b"machine-key-material"), dir.join("secrets.enc"));

        store
            .set_access_code("00M09A000000000", "11112222")
            .unwrap();
        store
            .set_access_code("00M09A000000001", "33334444")
            .unwrap();

        assert_eq!(store.access_code("00M09A000000000").unwrap(), "11112222");
        assert_eq!(store.access_code("00M09A000000001").unwrap(), "33334444");

        store.forget("00M09A000000000").unwrap();
        assert!(matches!(
            store.access_code("00M09A000000000"),
            Err(StoreError::SecretMissing(_))
        ));
        // The other secret is untouched.
        assert_eq!(store.access_code("00M09A000000001").unwrap(), "33334444");
    }

    #[test]
    fn an_unknown_serial_is_missing_not_an_error_variant() {
        let dir = TempDir::new();
        let store = EncryptedFileSecrets::new(material(b"key"), dir.join("secrets.enc"));

        assert!(matches!(
            store.access_code("nope"),
            Err(StoreError::SecretMissing(_))
        ));
    }

    #[test]
    fn wrong_key_material_cannot_decrypt() {
        let dir = TempDir::new();
        let path = dir.join("secrets.enc");

        EncryptedFileSecrets::new(material(b"the-right-key"), path.clone())
            .set_access_code("serial", "secret-code")
            .unwrap();

        // Same file, different machine: the AEAD tag rejects the wrong key.
        let intruder = EncryptedFileSecrets::new(material(b"the-wrong-key"), path);
        assert!(matches!(
            intruder.access_code("serial"),
            Err(StoreError::Corrupt { .. })
        ));
    }

    #[test]
    fn a_flipped_ciphertext_byte_is_detected() {
        let dir = TempDir::new();
        let path = dir.join("secrets.enc");
        let store = EncryptedFileSecrets::new(material(b"key"), path.clone());
        store.set_access_code("serial", "secret-code").unwrap();

        // Flip one character of the base64 ciphertext to another valid one:
        // the decoded bytes change, so the Poly1305 tag no longer matches.
        let mut sealed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let ct = sealed["ciphertext"].as_str().unwrap();
        let mut chars: Vec<char> = ct.chars().collect();
        chars[0] = if chars[0] == 'A' { 'B' } else { 'A' };
        sealed["ciphertext"] = serde_json::Value::String(chars.into_iter().collect());
        std::fs::write(&path, serde_json::to_vec(&sealed).unwrap()).unwrap();

        assert!(matches!(
            store.access_code("serial"),
            Err(StoreError::Corrupt { .. })
        ));
    }

    #[test]
    fn a_truncated_or_garbage_file_is_corrupt_not_empty() {
        let dir = TempDir::new();
        let path = dir.join("secrets.enc");
        std::fs::write(&path, b"not even json").unwrap();
        let store = EncryptedFileSecrets::new(material(b"key"), path);

        assert!(matches!(
            store.access_code("serial"),
            Err(StoreError::Corrupt { .. })
        ));
    }

    #[test]
    fn the_derived_key_is_not_the_raw_key_material() {
        let dir = TempDir::new();
        let raw = b"machine-key-material-that-is-32b!";
        let store = EncryptedFileSecrets::new(material(raw), dir.join("secrets.enc"));

        let key = store.derive_key(b"a-sixteen-byte!!").unwrap();

        assert_ne!(
            key.as_slice(),
            &raw[..],
            "argon2 must transform the material, not pass it through"
        );
    }

    #[test]
    fn each_write_uses_fresh_salt_and_nonce() {
        let dir = TempDir::new();
        let path = dir.join("secrets.enc");
        let store = EncryptedFileSecrets::new(material(b"key"), path.clone());

        store.set_access_code("serial", "code").unwrap();
        let first: SealedFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();

        store.set_access_code("serial", "code").unwrap();
        let second: SealedFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();

        // Same plaintext, but randomised salt+nonce make the file differ every
        // time — so ciphertext equality never leaks that a value is unchanged.
        assert_ne!(first.kdf_salt, second.kdf_salt);
        assert_ne!(first.nonce, second.nonce);
        assert_ne!(first.ciphertext, second.ciphertext);
    }
}
