use std::net::IpAddr;

use bambu_proto::{DeviceSerial, PrinterModel};

/// How a printer came to our attention.
///
/// Worth keeping: an SSDP hit and a subnet-probe hit warrant different
/// confidence, and a manually added printer must never be garbage collected
/// just because it went quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiscoverySource {
    Ssdp,
    SubnetProbe,
    Manual,
}

/// A printer we believe exists at an address.
///
/// Discovery is deliberately shallow: it answers "what is out there", not "what
/// is it doing". Everything past this point is [`bambu_client`]'s job.
///
/// [`bambu_client`]: https://docs.rs/bambu-client
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DiscoveredPrinter {
    pub address: IpAddr,
    /// `None` when the announcement did not include one — a subnet probe hit
    /// knows an address long before it knows a serial.
    pub serial: Option<DeviceSerial>,
    pub model: Option<PrinterModel>,
    pub source: DiscoverySource,
}

impl DiscoveredPrinter {
    /// A printer known only by address, as produced by a subnet probe.
    #[must_use]
    pub fn at(address: IpAddr, source: DiscoverySource) -> Self {
        Self {
            address,
            serial: None,
            model: None,
            source,
        }
    }
}
