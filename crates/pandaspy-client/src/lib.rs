//! Talking to a printer.
//!
//! One supervised session per printer: connect over TLS, authenticate,
//! subscribe to the report topic, fold reports into printer state, and
//! reconnect when the printer reboots mid-print (which it does). Watching
//! several printers means running several [`supervise`] tasks — nothing is
//! shared between them, so one failing printer cannot stall the others.
//!
//! # Trust
//!
//! Printers serve a self-signed certificate. There is no CA to check against,
//! so PandaSpy uses trust-on-first-use: remember the fingerprint the first
//! time, and treat a change as a hard stop the user must approve. See
//! [`pinning`] and [`TlsTransport`].
//!
//! # MQTT, hand-rolled
//!
//! The MQTT client is a small hand-written codec rather than a library: a
//! read-only monitor needs only a subset, and the alternative (rumqttc) drags
//! in aws-lc-rs, the system trust store, and a vulnerable transitive. The codec
//! is pure and the session loop is generic over its stream, so both are tested
//! without a socket.
//!
//! # What does not live here
//!
//! Payload parsing — that is [`pandaspy_proto`]. This crate moves bytes and
//! manages a connection's lifecycle; it does not know what a nozzle is.

mod backoff;
mod error;
mod jitter;
mod mqtt;
pub mod pinning;
mod session;
mod tls;

pub use backoff::Backoff;
pub use error::ClientError;
pub use jitter::{EqualJitter, Jitter, NoJitter};
pub use pinning::{CertificateFingerprint, PinStore, PinStoreError, TrustDecision, evaluate};
pub use session::{
    ConnectionState, Credentials, FailureReason, PrinterEndpoint, SessionConfig, SessionEvent,
    SessionSpec, Transport, TransportError, supervise,
};
pub use tls::TlsTransport;
