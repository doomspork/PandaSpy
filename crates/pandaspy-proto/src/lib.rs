//! Pure protocol layer for Bambu Lab printers.
//!
//! # What belongs here
//!
//! * the MQTT-carried wire contract: topics, request payloads, report
//!   classification ([`wire`])
//! * the state merge that folds a `pushall` snapshot plus sparse deltas into
//!   a [`PrinterState`] ([`StateAccumulator`], [`merge`])
//! * the typed model: job status, stages, AMS units and trays, HMS entries
//! * HMS/error text resolution from an embedded table snapshot ([`hms`]) —
//!   never fetched at runtime; offline-first is a privacy commitment
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
//! * every enum keeps an `Unknown(…)` variant that round-trips the raw value
//!   instead of failing
//! * numbers are parsed leniently — the same field arrives as `28.5` and
//!   `"28.5"` depending on model and firmware
//! * `#[serde(deny_unknown_fields)]` is banned
//!
//! See `CLAUDE.md` § Serde discipline. Any change to parsing behaviour
//! requires a fixture under `fixtures/` — no exceptions.

pub mod ams;
mod de;
mod error;
pub mod hms;
pub mod job;
pub mod merge;
mod model;
mod state;
pub mod wire;

pub use ams::{ActiveTray, AmsSystem, AmsUnit, AmsUnitType, Tray};
pub use error::ProtoError;
pub use hms::{HmsEntry, HmsModule, HmsSeverity};
pub use job::{GcodeState, PrintStage, PrinterStatus};
pub use model::{DeviceSerial, PrinterModel};
pub use state::{LightReport, NozzleType, PrinterState, StateAccumulator, fan_gear_to_percent};
pub use wire::{InfoReport, ModuleInfo, Report, ReportKind, Request};
