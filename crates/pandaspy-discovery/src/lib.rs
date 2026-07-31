//! Finding printers on the local network.
//!
//! Two strategies, in order of preference:
//!
//! 1. **SSDP** — printers announce on `239.255.255.250:2021` (Bambu's port,
//!    not the standard 1900), and answer M-SEARCH queries. Discovery does
//!    both at once: a passive listener catches unsolicited NOTIFYs while
//!    per-interface search sockets ask directly.
//! 2. **Subnet probe** — knock on TCP 8883 across the local subnet and read
//!    the TLS certificate; its CN is the device serial. Loud and slow, so it
//!    is the fallback, never the front door.
//!
//! # The multi-interface rule
//!
//! The passive listener joins the multicast group on **every** usable
//! interface, and every usable interface gets its own search socket. On a
//! machine with a VPN, a Docker bridge and two NICs, "just use the default
//! interface" discovers nothing and reports nothing — that silent failure is
//! the most common one this crate exists to prevent.
//!
//! # Diagnostics are a product feature
//!
//! Zero printers found is an answer that must come with an explanation.
//! Every run returns [`DiscoveryDiagnostics`] recording what was tried and
//! what each step said, collapsed by [`DiscoveryDiagnostics::verdict`] into
//! the [`DiscoveryVerdict`] the troubleshooting UI shows: no usable
//! interface vs permission denied vs nothing answered.
//!
//! # The trait seam
//!
//! The engine ([`discover`]) is generic over [`SsdpStack`], [`SsdpSocket`],
//! [`PortProbe`] and [`InterfaceSource`]. Tests drive it with scripted fakes
//! under a paused tokio clock; the one real, fully portable implementation
//! lives in [`net`]. `#[cfg(target_os)]` is banned here — a platform that
//! needs different socket setup gets it inside a cross-platform dependency
//! or from an implementation injected by `src-tauri`, never from a branch in
//! the algorithm.

pub mod diag;
mod discover;
mod interfaces;
pub mod net;
mod probe;
pub mod ssdp;
pub mod subnet;
mod types;

pub use diag::{DiscoveryDiagnostics, DiscoveryVerdict, IoFailure};
pub use discover::{
    DiscoveryOptions, DiscoveryOutcome, PassiveSetup, ProbePolicy, SsdpStack, discover,
};
pub use interfaces::{InterfaceDecision, InterfaceSource, NetInterface, assess};
pub use probe::{PortProbe, ProbeOutcome, serial_from_cert};
pub use ssdp::{SsdpAnnouncement, SsdpMessageKind, SsdpSocket};
pub use types::{DiscoveredPrinter, DiscoverySource};
