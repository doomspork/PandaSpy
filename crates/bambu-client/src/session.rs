use std::fmt;
use std::net::IpAddr;

use bambu_proto::{DeviceSerial, PrinterState};

use crate::pinning::CertificateFingerprint;

/// Where to reach a printer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrinterEndpoint {
    pub address: IpAddr,
    pub port: u16,
    pub serial: DeviceSerial,
}

/// What the printer wants before it will talk.
///
/// The access code is printed on the printer's screen and is effectively a
/// password. It is held here only for the duration of a connect; the durable
/// copy lives in the OS secret store via `bambu-store`.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub access_code: String,
}

impl fmt::Debug for Credentials {
    /// Hand-written so an access code cannot reach a log file, a crash report
    /// or a GitHub issue by way of a stray `{:?}`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("access_code", &"<redacted>")
            .finish()
    }
}

/// Everything a session tells the rest of the app.
///
/// Modelled as events rather than a polled struct so the tray, the window and
/// the notifier can each react to exactly what they care about.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SessionEvent {
    Connected,
    /// A full or partial report. Callers fold it in with
    /// [`PrinterState::merge_from`].
    Report(Box<PrinterState>),
    Disconnected {
        /// Human-readable, already localised by the time it reaches the UI.
        reason: String,
    },
    /// The session is blocked until the user approves a changed certificate.
    /// Deliberately an event and not a prompt: this crate must not know that
    /// dialogs exist.
    TrustDecisionRequired {
        serial: DeviceSerial,
        pinned: CertificateFingerprint,
        presented: CertificateFingerprint,
    },
}

// TODO(scaffold): the session itself. Expected shape — a struct owning the MQTT
// connection plus a `Backoff`, driven by a loop that emits `SessionEvent`s on a
// channel. Keep the reconnect policy separable from the transport so it can be
// tested without a socket.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_contains_the_access_code() {
        let credentials = Credentials {
            username: "bblp".to_owned(),
            access_code: "12345678".to_owned(),
        };

        let rendered = format!("{credentials:?}");

        assert!(!rendered.contains("12345678"), "leaked: {rendered}");
        assert!(rendered.contains("<redacted>"));
    }
}
