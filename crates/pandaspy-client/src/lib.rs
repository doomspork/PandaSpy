//! Talking to a printer.
//!
//! One [`Session`]-shaped thing per printer: connect over TLS, authenticate,
//! subscribe to the report topic, hand decoded state to the caller, and
//! reconnect when the printer reboots mid-print (which it does).
//!
//! # Trust
//!
//! Printers serve a self-signed certificate. There is no CA to check against,
//! so PandaSpy uses trust-on-first-use: remember the fingerprint the first time,
//! and treat a change as a hard stop that the user must approve. See
//! [`pinning`].
//!
//! # What does not live here
//!
//! Payload parsing — that is [`pandaspy_proto`]. This crate moves bytes and
//! manages a connection's lifecycle; it does not know what a nozzle is.

mod backoff;
mod error;
pub mod pinning;
mod session;

pub use backoff::Backoff;
pub use error::ClientError;
pub use session::{Credentials, PrinterEndpoint, SessionEvent};
