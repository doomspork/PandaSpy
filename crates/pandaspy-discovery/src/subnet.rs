//! Planning the subnet probe: which addresses are worth knocking on.
//!
//! Pure arithmetic — the plan is computed from interface addresses and
//! netmasks, and the walk itself happens elsewhere. Two guardrails keep the
//! fallback from becoming a port scanner:
//!
//! * Anything wider than a `/24` is **clamped to the `/24` containing our
//!   own address**. A corporate `/16` is 65k hosts; probing all of them is
//!   rude, slow, and — for a printer that is almost always on the same
//!   segment as the machine running PandaSpy — pointless. The clamp is
//!   recorded in the plan so the UI can say "we only checked your slice".
//! * A total-target cap, with a `truncated` flag when it bites.

use std::net::Ipv4Addr;

use serde::Serialize;

use crate::interfaces::{NetInterface, assess};

/// The addresses the probe will try, plus what was left out and why.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ProbePlan {
    pub targets: Vec<Ipv4Addr>,
    /// Interfaces whose subnet was wider than /24 and got clamped.
    pub clamped: Vec<String>,
    /// True when the total cap cut the list short.
    pub truncated: bool,
}

/// Build the probe plan for a set of interfaces.
///
/// `cap` bounds the total number of targets across all interfaces.
#[must_use]
pub fn plan(interfaces: &[NetInterface], cap: usize) -> ProbePlan {
    let mut plan = ProbePlan::default();
    let own: Vec<Ipv4Addr> = interfaces.iter().map(|iface| iface.ip).collect();
    let mut seen = std::collections::BTreeSet::new();

    for interface in interfaces {
        if !assess(interface).usable() {
            continue;
        }
        let Some(prefix) = prefix_len(interface.netmask) else {
            continue; // non-contiguous mask: nothing sane to walk
        };

        // Clamp anything wider than /24 to the /24 around our own address.
        let effective_prefix = if prefix < 24 {
            plan.clamped.push(interface.name.clone());
            24
        } else {
            prefix
        };
        if effective_prefix == 32 {
            continue; // a host route: no peer to find
        }

        // For a /31 (RFC 3021 point-to-point) both addresses are hosts — there
        // is no network or broadcast address to exclude, so the range is the
        // whole block. For everything /30 and wider, drop the network and
        // broadcast addresses at the ends.
        let mask = u32::MAX << (32 - effective_prefix);
        let network = u32::from(interface.ip) & mask;
        let broadcast = network | !mask;
        let (first, last) = if effective_prefix == 31 {
            (network, broadcast)
        } else {
            (network + 1, broadcast - 1)
        };

        for host in first..=last {
            let address = Ipv4Addr::from(host);
            if own.contains(&address) || !seen.insert(address) {
                continue;
            }
            if plan.targets.len() >= cap {
                plan.truncated = true;
                return plan;
            }
            plan.targets.push(address);
        }
    }

    plan
}

/// Netmask → prefix length, `None` for non-contiguous garbage.
#[must_use]
pub fn prefix_len(netmask: Ipv4Addr) -> Option<u8> {
    let bits = u32::from(netmask);
    let ones = bits.count_ones();
    // Contiguous means: all set bits are the topmost ones.
    if bits.leading_ones() == ones {
        Some(ones as u8)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(name: &str, ip: [u8; 4], mask: [u8; 4]) -> NetInterface {
        NetInterface {
            name: name.to_owned(),
            ip: Ipv4Addr::from(ip),
            netmask: Ipv4Addr::from(mask),
        }
    }

    #[test]
    fn a_slash_24_yields_253_targets_without_self_network_or_broadcast() {
        let plan = plan(&[iface("en0", [192, 168, 0, 10], [255, 255, 255, 0])], 4096);

        assert_eq!(plan.targets.len(), 253, "254 hosts minus ourselves");
        assert!(
            !plan.targets.contains(&Ipv4Addr::new(192, 168, 0, 10)),
            "self"
        );
        assert!(
            !plan.targets.contains(&Ipv4Addr::new(192, 168, 0, 0)),
            "network"
        );
        assert!(
            !plan.targets.contains(&Ipv4Addr::new(192, 168, 0, 255)),
            "broadcast"
        );
        assert!(plan.clamped.is_empty());
        assert!(!plan.truncated);
    }

    #[test]
    fn wide_subnets_clamp_to_our_own_slash_24() {
        // A /16 would be 65534 hosts. The plan walks only the /24 around us
        // and says so.
        let plan = plan(&[iface("corp0", [10, 20, 30, 40], [255, 255, 0, 0])], 4096);

        assert_eq!(plan.targets.len(), 253);
        assert!(
            plan.targets
                .iter()
                .all(|ip| ip.octets()[..3] == [10, 20, 30])
        );
        assert_eq!(plan.clamped, vec!["corp0".to_owned()]);
    }

    #[test]
    fn the_cap_truncates_and_admits_it() {
        let plan = plan(&[iface("en0", [192, 168, 0, 10], [255, 255, 255, 0])], 100);
        assert_eq!(plan.targets.len(), 100);
        assert!(plan.truncated);
    }

    #[test]
    fn overlapping_interfaces_do_not_probe_twice() {
        let plan = plan(
            &[
                iface("en0", [192, 168, 0, 10], [255, 255, 255, 0]),
                iface("en1", [192, 168, 0, 20], [255, 255, 255, 0]),
            ],
            4096,
        );
        // Same /24 from both: deduplicated, and neither of our own addresses
        // is in the list.
        assert_eq!(plan.targets.len(), 252);
    }

    #[test]
    fn unusable_interfaces_contribute_nothing() {
        let plan = plan(
            &[
                iface("lo0", [127, 0, 0, 1], [255, 0, 0, 0]),
                iface("awdl0", [169, 254, 3, 4], [255, 255, 0, 0]),
            ],
            4096,
        );
        assert!(plan.targets.is_empty());
    }

    #[test]
    fn a_slash_31_probes_its_single_peer() {
        // RFC 3021 point-to-point: both addresses are hosts, so the one that
        // is not ours is worth a knock.
        let plan = plan(&[iface("p2p0", [10, 0, 0, 1], [255, 255, 255, 254])], 4096);
        assert_eq!(plan.targets, vec![Ipv4Addr::new(10, 0, 0, 0)]);
    }

    #[test]
    fn host_routes_and_broken_masks_are_skipped() {
        // /32 has no peer; a non-contiguous mask is garbage.
        let plan = plan(
            &[
                iface("lo-ish", [10, 0, 0, 1], [255, 255, 255, 255]),
                iface("odd0", [10, 0, 1, 1], [255, 0, 255, 0]),
            ],
            4096,
        );
        assert!(plan.targets.is_empty());
    }

    #[test]
    fn a_slash_30_yields_its_two_hosts_minus_self() {
        // 4 addresses, 2 usable hosts, one of them ours.
        let plan = plan(
            &[iface("p2p1", [192, 168, 5, 1], [255, 255, 255, 252])],
            4096,
        );
        assert_eq!(plan.targets, vec![Ipv4Addr::new(192, 168, 5, 2)]);
    }

    #[test]
    fn the_cap_boundary_does_not_truncate_when_it_exactly_fits() {
        // cap == target count is the off-by-one that a `>=` in the wrong place
        // would trip: 253 hosts, cap 253, nothing dropped.
        let plan = plan(&[iface("en0", [192, 168, 0, 10], [255, 255, 255, 0])], 253);
        assert_eq!(plan.targets.len(), 253);
        assert!(!plan.truncated);
    }

    #[test]
    fn prefix_lengths_decode_only_contiguous_masks() {
        assert_eq!(prefix_len(Ipv4Addr::new(255, 255, 255, 0)), Some(24));
        assert_eq!(prefix_len(Ipv4Addr::new(255, 255, 0, 0)), Some(16));
        assert_eq!(prefix_len(Ipv4Addr::new(0, 0, 0, 0)), Some(0));
        assert_eq!(prefix_len(Ipv4Addr::new(255, 0, 255, 0)), None);
    }
}
