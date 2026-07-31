//! Network interfaces, and the decision of which ones discovery should use.
//!
//! **This is the part that fails silently everywhere else.** On a machine
//! with a VPN, a Docker bridge and two NICs, joining the multicast group on
//! only "the default interface" discovers nothing and says nothing. The rule
//! here: enumerate every interface, join on every suitable one, and record a
//! reason for every one that was skipped — the skip reasons are diagnostics,
//! not discards.

use std::io;
use std::net::Ipv4Addr;

use serde::Serialize;

/// One IPv4 interface address as discovery sees it.
///
/// IPv4 only, deliberately: Bambu's SSDP announcer speaks IPv4 multicast.
/// Enumerators drop IPv6 addresses before they reach this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NetInterface {
    /// OS name (`en0`, `eth0`, `Ethernet 2`) — shown in diagnostics.
    pub name: String,
    pub ip: Ipv4Addr,
    pub netmask: Ipv4Addr,
}

/// Where interfaces come from. Synchronous: enumeration is a local syscall,
/// and keeping it sync lets fakes be plain vectors.
pub trait InterfaceSource: Send + Sync + std::fmt::Debug {
    /// Every IPv4 interface address on the machine, loopback included —
    /// filtering is [`assess`]'s job, so the diagnostics can show what was
    /// rejected and why.
    fn interfaces(&self) -> io::Result<Vec<NetInterface>>;
}

/// Why an interface is or is not worth joining the multicast group on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum InterfaceDecision {
    /// Join and search here.
    Use,
    /// `127.0.0.0/8` — no printer lives there.
    SkipLoopback,
    /// `169.254.0.0/16` — self-assigned, means "DHCP failed"; a subnet walk
    /// of it would probe 65k addresses that cannot answer.
    SkipLinkLocal,
    /// `0.0.0.0` or a mask that describes no network.
    SkipUnusable,
}

impl InterfaceDecision {
    #[must_use]
    pub fn usable(self) -> bool {
        self == Self::Use
    }
}

/// Classify one interface.
#[must_use]
pub fn assess(interface: &NetInterface) -> InterfaceDecision {
    let ip = interface.ip;
    if ip.is_loopback() {
        return InterfaceDecision::SkipLoopback;
    }
    if ip.is_link_local() {
        return InterfaceDecision::SkipLinkLocal;
    }
    if ip.is_unspecified() || u32::from(interface.netmask) == 0 {
        return InterfaceDecision::SkipUnusable;
    }
    InterfaceDecision::Use
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(ip: [u8; 4], mask: [u8; 4]) -> NetInterface {
        NetInterface {
            name: "test0".to_owned(),
            ip: Ipv4Addr::from(ip),
            netmask: Ipv4Addr::from(mask),
        }
    }

    #[test]
    fn ordinary_lan_interfaces_are_used() {
        assert_eq!(
            assess(&iface([192, 168, 0, 10], [255, 255, 255, 0])),
            InterfaceDecision::Use
        );
        // A VPN's odd prefix is still a network; printers can live behind
        // routed segments. Use it — the multicast join just may not deliver.
        assert_eq!(
            assess(&iface([10, 8, 0, 2], [255, 255, 0, 0])),
            InterfaceDecision::Use
        );
    }

    #[test]
    fn loopback_link_local_and_unspecified_are_skipped_with_reasons() {
        assert_eq!(
            assess(&iface([127, 0, 0, 1], [255, 0, 0, 0])),
            InterfaceDecision::SkipLoopback
        );
        assert_eq!(
            assess(&iface([169, 254, 12, 7], [255, 255, 0, 0])),
            InterfaceDecision::SkipLinkLocal
        );
        assert_eq!(
            assess(&iface([0, 0, 0, 0], [0, 0, 0, 0])),
            InterfaceDecision::SkipUnusable
        );
        assert_eq!(
            assess(&iface([192, 168, 0, 10], [0, 0, 0, 0])),
            InterfaceDecision::SkipUnusable
        );
    }
}
