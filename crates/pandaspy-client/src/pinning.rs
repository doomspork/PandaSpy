//! Trust-on-first-use certificate pinning.
//!
//! Bambu printers serve a self-signed certificate generated on the device.
//! There is no chain to validate, so ordinary TLS verification is meaningless:
//! it either rejects every printer or (if disabled) accepts every impostor.
//!
//! Instead: record the fingerprint the first time a printer is seen, and
//! require it to match forever after. A mismatch is surfaced to the user, never
//! silently accepted and never silently retried.

use std::fmt;

use pandaspy_proto::DeviceSerial;

/// SHA-256 of the printer's DER-encoded leaf certificate.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CertificateFingerprint(pub [u8; 32]);

impl fmt::Debug for CertificateFingerprint {
    /// Hex, because a byte array in a log line is unreadable and the user may
    /// be asked to compare this against what the printer's screen shows.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Display for CertificateFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// The verdict on a certificate presented by a printer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrustDecision {
    /// Matches what we pinned. Proceed.
    Trusted,
    /// Never seen this printer before. Pin it and proceed — that is what
    /// "on first use" means.
    FirstSight(CertificateFingerprint),
    /// Seen before, and it changed. Stop and ask the user. A firmware reflash
    /// looks exactly like an attacker from here, and only they can tell.
    Changed {
        pinned: CertificateFingerprint,
        presented: CertificateFingerprint,
    },
}

/// Where pins are remembered.
///
/// Synchronous on purpose: reading a pin is a hash-map or small-file lookup on
/// the connect path, and making it `async` would force a runtime into code that
/// does not otherwise need one.
///
/// The persistent implementation lives in `pandaspy-store`, which is the crate
/// allowed to know what a keychain is.
pub trait PinStore: Send + Sync + fmt::Debug {
    /// The fingerprint pinned for this printer, if any.
    fn pinned(&self, serial: &DeviceSerial) -> Option<CertificateFingerprint>;

    /// Record a fingerprint, replacing any previous one.
    ///
    /// Callers must only reach this after either first sight or explicit user
    /// approval of a change.
    fn pin(
        &self,
        serial: &DeviceSerial,
        fingerprint: CertificateFingerprint,
    ) -> Result<(), PinStoreError>;
}

/// A pin could not be persisted.
#[derive(Debug, thiserror::Error)]
#[error("could not persist certificate pin: {0}")]
pub struct PinStoreError(pub String);

/// Compare a presented certificate against what we know.
///
/// Pure: given a store and a fingerprint, the verdict is total and has no side
/// effects. Persisting the result is the caller's decision, which is what makes
/// "ask the user first" expressible.
#[must_use]
pub fn evaluate<S: PinStore + ?Sized>(
    store: &S,
    serial: &DeviceSerial,
    presented: CertificateFingerprint,
) -> TrustDecision {
    match store.pinned(serial) {
        None => TrustDecision::FirstSight(presented),
        Some(pinned) if pinned == presented => TrustDecision::Trusted,
        Some(pinned) => TrustDecision::Changed { pinned, presented },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug, Default)]
    struct MemoryPins(Mutex<HashMap<String, CertificateFingerprint>>);

    impl PinStore for MemoryPins {
        fn pinned(&self, serial: &DeviceSerial) -> Option<CertificateFingerprint> {
            self.0.lock().unwrap().get(&serial.0).copied()
        }

        fn pin(
            &self,
            serial: &DeviceSerial,
            fingerprint: CertificateFingerprint,
        ) -> Result<(), PinStoreError> {
            self.0.lock().unwrap().insert(serial.0.clone(), fingerprint);
            Ok(())
        }
    }

    fn serial() -> DeviceSerial {
        DeviceSerial("TEST0000000000".to_owned())
    }

    #[test]
    fn an_unknown_printer_is_first_sight_not_a_failure() {
        let store = MemoryPins::default();
        let presented = CertificateFingerprint([1; 32]);

        assert_eq!(
            evaluate(&store, &serial(), presented),
            TrustDecision::FirstSight(presented)
        );
    }

    #[test]
    fn a_matching_certificate_is_trusted() {
        let store = MemoryPins::default();
        let fingerprint = CertificateFingerprint([2; 32]);
        store.pin(&serial(), fingerprint).unwrap();

        assert_eq!(
            evaluate(&store, &serial(), fingerprint),
            TrustDecision::Trusted
        );
    }

    #[test]
    fn a_changed_certificate_reports_both_fingerprints_for_the_user_to_compare() {
        let store = MemoryPins::default();
        let pinned = CertificateFingerprint([3; 32]);
        let presented = CertificateFingerprint([4; 32]);
        store.pin(&serial(), pinned).unwrap();

        assert_eq!(
            evaluate(&store, &serial(), presented),
            TrustDecision::Changed { pinned, presented }
        );
    }

    #[test]
    fn fingerprints_render_as_hex() {
        let fingerprint = CertificateFingerprint([0xab; 32]);
        assert_eq!(fingerprint.to_string(), "ab".repeat(32));
    }
}
