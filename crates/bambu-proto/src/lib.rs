//! Pure protocol layer for Bambu Lab printers.
//!
//! # What belongs here
//!
//! * wire types for the MQTT report/request payloads
//! * the codec that turns those payloads into domain types and back
//! * the state merge that folds partial reports into a full [`PrinterState`]
//! * the HMS (Health Management System) error-code table
//!
//! # What does not
//!
//! Anything that touches the outside world. This crate has no sockets, no
//! files, no clock, no randomness, no async runtime and no `#[cfg(target_os)]`.
//! Given the same bytes it must produce the same value on every platform,
//! forever. CI enforces that by compiling this crate for
//! `wasm32-unknown-unknown`, where none of those facilities exist.
//!
//! # Parsing discipline
//!
//! Bambu's protocol is undocumented, differs between printer models, and
//! changes without notice across firmware releases. A parser that rejects
//! surprises is a parser that bricks the app on the next firmware drop, so:
//!
//! * every field is `Option<T>` — absence is normal, not an error
//! * every enum keeps an `Unknown(String)` variant that round-trips the raw
//!   value instead of failing
//! * `#[serde(deny_unknown_fields)]` is banned
//!
//! See `CLAUDE.md` § Serde discipline.

mod error;
mod hms;
mod model;
mod state;

pub use error::ProtoError;
pub use hms::{HmsCode, HmsSeverity, lookup_hms};
pub use model::{DeviceSerial, PrinterModel};
pub use state::{JobStage, PrinterState};
