//! Finding printers on the local network.
//!
//! Two strategies, in order of preference:
//!
//! 1. **SSDP** — printers announce themselves over multicast. Fast and cheap
//!    when it works, but multicast is routinely dropped by consumer APs, VPN
//!    clients and "AP isolation" settings.
//! 2. **Subnet probe** — walk the local /24 looking for the printer's TCP
//!    port. Slow and noisy, so it is the fallback, never the default.
//!
//! # The trait seam
//!
//! Neither strategy owns a socket. Both are written against [`SsdpSocket`] and
//! [`PortProbe`], which `src-tauri` implements with real sockets and tests
//! implement with canned data. That is what keeps this crate's logic testable
//! without a printer, a network, or an async runtime in the test binary.
//!
//! It is also why `#[cfg(target_os)]` is banned here: if a platform needs a
//! different socket setup, that belongs in the *implementation* of these
//! traits, not in the algorithm that consumes them.

mod error;
mod probe;
mod ssdp;
mod types;

pub use error::DiscoveryError;
pub use probe::PortProbe;
pub use ssdp::SsdpSocket;
pub use types::{DiscoveredPrinter, DiscoverySource};
