use std::net::IpAddr;

use pandaspy_proto::{DeviceSerial, PrinterModel};
use serde::Serialize;

/// How a printer came to our attention.
///
/// Worth keeping: an SSDP hit and a subnet-probe hit warrant different
/// confidence, and a manually added printer must never be garbage collected
/// just because it went quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[non_exhaustive]
pub enum DiscoverySource {
    Ssdp,
    SubnetProbe,
    Manual,
}

/// A printer we believe exists at an address.
///
/// Discovery is deliberately shallow: it answers "what is out there", not
/// "what is it doing". Everything past this point is `pandaspy-client`'s job.
///
/// Serializable because the add-printer UI renders these directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct DiscoveredPrinter {
    pub address: IpAddr,
    /// `None` when the evidence did not include one — an SSDP announcement
    /// missing its `USN`, for instance. A subnet-probe hit always has one
    /// (the serial *is* the certificate CN that made it a hit).
    pub serial: Option<DeviceSerial>,
    pub model: Option<PrinterModel>,
    /// The user's name for the printer (`DevName.bambu.com`).
    pub name: Option<String>,
    /// Printer-side Wi-Fi RSSI in dBm (`DevSignal.bambu.com`).
    pub signal_dbm: Option<i16>,
    /// `"lan"` or `"cloud"` (`DevConnect.bambu.com`).
    pub connect_mode: Option<String>,
    /// `"free"` or a binding state (`DevBind.bambu.com`).
    pub bind_state: Option<String>,
    pub source: DiscoverySource,
}

impl DiscoveredPrinter {
    /// A printer known only by address and serial, as produced by the
    /// subnet probe.
    #[must_use]
    pub fn from_probe(address: IpAddr, serial: DeviceSerial) -> Self {
        Self {
            address,
            serial: Some(serial),
            model: None,
            name: None,
            signal_dbm: None,
            connect_mode: None,
            bind_state: None,
            source: DiscoverySource::SubnetProbe,
        }
    }

    /// The identity used for deduplication: serial when known, else address.
    /// Two sightings of the same serial are one printer even if DHCP moved
    /// it between interfaces mid-scan.
    #[must_use]
    pub fn dedup_key(&self) -> String {
        match &self.serial {
            Some(serial) => format!("sn:{serial}"),
            None => format!("ip:{}", self.address),
        }
    }

    /// Fold a second sighting of the same printer into this one, filling
    /// gaps without overwriting richer evidence. SSDP metadata (name, model,
    /// signal) wins over a bare probe hit regardless of arrival order.
    pub fn absorb(&mut self, other: &Self) {
        if self.serial.is_none() {
            self.serial = other.serial.clone();
        }
        if self.model.is_none() {
            self.model = other.model.clone();
        }
        if self.name.is_none() {
            self.name = other.name.clone();
        }
        if self.signal_dbm.is_none() {
            self.signal_dbm = other.signal_dbm;
        }
        if self.connect_mode.is_none() {
            self.connect_mode = other.connect_mode.clone();
        }
        if self.bind_state.is_none() {
            self.bind_state = other.bind_state.clone();
        }
        // An SSDP sighting is stronger provenance than a probe.
        if self.source == DiscoverySource::SubnetProbe && other.source == DiscoverySource::Ssdp {
            self.source = DiscoverySource::Ssdp;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_hit() -> DiscoveredPrinter {
        DiscoveredPrinter::from_probe(
            "192.168.0.2".parse().unwrap(),
            DeviceSerial("00M09A000000000".to_owned()),
        )
    }

    #[test]
    fn dedup_prefers_serial_over_address() {
        let by_serial = probe_hit();
        assert_eq!(by_serial.dedup_key(), "sn:00M09A000000000");

        let mut anonymous = probe_hit();
        anonymous.serial = None;
        assert_eq!(anonymous.dedup_key(), "ip:192.168.0.2");
    }

    #[test]
    fn absorbing_an_ssdp_sighting_upgrades_a_probe_hit() {
        let mut printer = probe_hit();
        let richer = DiscoveredPrinter {
            address: printer.address,
            serial: printer.serial.clone(),
            model: Some(PrinterModel::P1S),
            name: Some("workshop".to_owned()),
            signal_dbm: Some(-51),
            connect_mode: Some("lan".to_owned()),
            bind_state: Some("free".to_owned()),
            source: DiscoverySource::Ssdp,
        };

        printer.absorb(&richer);

        assert_eq!(printer.model, Some(PrinterModel::P1S));
        assert_eq!(printer.name.as_deref(), Some("workshop"));
        assert_eq!(printer.source, DiscoverySource::Ssdp);
    }

    #[test]
    fn absorbing_a_poorer_sighting_changes_nothing() {
        let mut printer = probe_hit();
        printer.model = Some(PrinterModel::P1S);
        printer.source = DiscoverySource::Ssdp;

        printer.absorb(&probe_hit());

        assert_eq!(printer.model, Some(PrinterModel::P1S));
        assert_eq!(printer.source, DiscoverySource::Ssdp);
    }
}
