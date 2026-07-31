//! Structured discovery diagnostics.
//!
//! An empty printer list is the *start* of the answer, not the end of it.
//! Every mechanical step of a discovery run — interface classification,
//! multicast joins, search transmissions, datagram counts, probe coverage —
//! is recorded here, and [`DiscoveryDiagnostics::verdict`] collapses the
//! evidence into the distinction the troubleshooting UI needs: *no usable
//! interface* vs *permission denied* vs *nothing answered*.
//!
//! Everything is `Serialize` and stringly-typed where errors are involved:
//! this structure crosses the Tauri event bridge verbatim and lands in a UI.

use serde::Serialize;

use crate::interfaces::{InterfaceDecision, NetInterface};

/// An I/O failure flattened into something serializable and comparable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IoFailure {
    /// `std::io::ErrorKind` in its `Debug` spelling, e.g. `PermissionDenied`.
    pub kind: String,
    pub message: String,
}

impl IoFailure {
    #[must_use]
    pub fn from_io(error: &std::io::Error) -> Self {
        Self {
            kind: format!("{:?}", error.kind()),
            message: error.to_string(),
        }
    }

    /// Whether this failure smells like an OS-level permission refusal.
    ///
    /// `PermissionDenied` is the honest signal. `HostUnreachable` on a
    /// *multicast send* is included because that is the documented symptom
    /// of macOS 14+ Local Network permission being denied: the OS does not
    /// say "forbidden", it pretends there is no route.
    /// TODO(fixture): confirm the exact macOS symptom from a bundled build
    /// once M5 wires `NSLocalNetworkUsageDescription`.
    #[must_use]
    pub fn looks_like_permission_denial(&self) -> bool {
        self.kind == "PermissionDenied" || self.kind == "HostUnreachable"
    }
}

/// One interface, and everything that happened on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InterfaceReport {
    pub interface: NetInterface,
    pub decision: InterfaceDecision,
    /// Multicast group join on the passive socket. `None` = not attempted
    /// (interface skipped, or the passive socket never bound).
    pub join: Option<Result<(), IoFailure>>,
    /// Opening the per-interface search socket.
    pub search_socket: Option<Result<(), IoFailure>>,
}

/// The passive listener (bound to the SSDP port, joined on every usable
/// interface).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PassiveReport {
    /// `Some(failure)` when binding the SSDP port failed — commonly
    /// `AddrInUse` because Bambu Studio is running. Active search still
    /// works in that case; this is context, not a dead end, and the verdict
    /// treats an `AddrInUse` bind failure as neutral for exactly that reason.
    pub bind_failure: Option<IoFailure>,
    pub datagrams: u32,
    /// Non-terminal recv errors (see [`crate::discover`]). Diagnostics only —
    /// a per-datagram artefact like the Windows ICMP reset is not evidence of
    /// anything the verdict should weigh.
    pub recv_failures: Vec<IoFailure>,
}

/// The active M-SEARCH side.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ActiveReport {
    pub searches_sent: u32,
    pub send_failures: Vec<IoFailure>,
    pub datagrams: u32,
    /// Non-terminal recv errors on search sockets; diagnostics only.
    pub recv_failures: Vec<IoFailure>,
}

/// What the datagrams contained, across both sockets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ParseReport {
    /// NOTIFY or search responses that described a printer.
    pub announcements: u32,
    /// M-SEARCH queries seen (usually our own multicast echo).
    pub msearch_echoes: u32,
    /// Datagrams that were not SSDP, or announcements naming nothing.
    pub ignored: u32,
}

/// The subnet-probe stage. `None` when the stage never ran.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ProbeReport {
    pub planned: usize,
    pub clamped: Vec<String>,
    pub truncated: bool,
    pub probed: usize,
    /// Endpoints that completed TLS and presented a certificate.
    pub tls_peers: u32,
    /// Certificates whose CN read as a printer serial.
    pub serials_read: u32,
}

/// The full mechanical record of one discovery run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DiscoveryDiagnostics {
    /// Set when the OS refused to even list interfaces; the run is over
    /// before it starts and `interfaces` will be empty.
    pub enumeration_failure: Option<IoFailure>,
    pub interfaces: Vec<InterfaceReport>,
    pub passive: PassiveReport,
    pub active: ActiveReport,
    pub parse: ParseReport,
    pub probe: Option<ProbeReport>,
}

/// The one-line answer for the UI when the printer list is empty — and the
/// confirmation when it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum DiscoveryVerdict {
    /// At least one printer was found.
    Found,
    /// There was no interface worth trying: no network, or nothing but
    /// loopback and self-assigned addresses. Plugging into a network is the
    /// fix; retrying is not.
    NoUsableInterface,
    /// The OS refused the sockets themselves — joins, binds or sends failed
    /// with permission-shaped errors on every interface that existed. On
    /// macOS 14+ this is the Local Network permission prompt having been
    /// dismissed or denied.
    PermissionDenied,
    /// Everything worked mechanically — sockets opened, searches left,
    /// probes ran — and the network stayed silent. The printer is off, on a
    /// different network, or multicast-filtered *and* outside the probed
    /// range.
    NoResponse,
}

impl DiscoveryVerdict {
    /// Stable identifier for Fluent localisation, mirroring
    /// `PrintStage::key` in `pandaspy-proto`.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Found => "found",
            Self::NoUsableInterface => "no-usable-interface",
            Self::PermissionDenied => "permission-denied",
            Self::NoResponse => "no-response",
        }
    }
}

impl DiscoveryDiagnostics {
    /// Collapse the evidence into a verdict, given how many printers the run
    /// produced.
    ///
    /// Precedence when empty-handed: permission trouble beats "no usable
    /// interface" beats silence — each earlier verdict has a more specific
    /// user action attached, and permission failures on real interfaces
    /// must never be misreported as an empty network.
    #[must_use]
    pub fn verdict(&self, printers_found: usize) -> DiscoveryVerdict {
        if printers_found > 0 {
            return DiscoveryVerdict::Found;
        }

        let usable: Vec<&InterfaceReport> = self
            .interfaces
            .iter()
            .filter(|report| report.decision.usable())
            .collect();

        if !usable.is_empty() && self.everything_permission_shaped(&usable) {
            return DiscoveryVerdict::PermissionDenied;
        }
        if usable.is_empty() {
            return DiscoveryVerdict::NoUsableInterface;
        }
        DiscoveryVerdict::NoResponse
    }

    /// True when the OS is the obstacle: at least one permission-shaped
    /// failure, no failure of any *other* shape, and zero evidence of
    /// working network I/O.
    ///
    /// What counts as evidence matters here. A successfully *opened* socket
    /// or a successful group join proves nothing — those are local kernel
    /// state, and macOS happily grants both while denying actual traffic
    /// under its Local Network permission. Only transmitted searches and
    /// received datagrams demonstrate a working path.
    fn everything_permission_shaped(&self, usable: &[&InterfaceReport]) -> bool {
        // Anything actually moved? Then permission is not the problem.
        if self.passive.datagrams > 0 || self.active.datagrams > 0 {
            return false;
        }
        if self.active.searches_sent > 0 {
            return false;
        }
        // The subnet probe is a second, independent network path. A completed
        // TLS handshake there is unambiguous proof that traffic flows in both
        // directions — so if the probe reached any TLS peer, the OS is not
        // blocking us, whatever SSDP did. (SSDP is UDP multicast and the probe
        // is unicast TCP: macOS Local Network permission gates both, so a
        // genuine denial shows zero here too.)
        if let Some(probe) = &self.probe
            && probe.tls_peers > 0
        {
            return false;
        }

        let mut denials = 0_u32;
        let mut other_failures = 0_u32;
        let mut tally = |failure: &IoFailure| {
            if failure.looks_like_permission_denial() {
                denials += 1;
            } else {
                other_failures += 1;
            }
        };

        for report in usable {
            if let Some(Err(failure)) = &report.join {
                tally(failure);
            }
            if let Some(Err(failure)) = &report.search_socket {
                tally(failure);
            }
        }
        for failure in &self.active.send_failures {
            tally(failure);
        }
        // A passive bind that failed `AddrInUse` is neutral, not a dilutant:
        // it means another SSDP listener (almost always Bambu Studio) holds
        // port 2021, which says nothing about whether the OS is blocking us.
        // Counting it as an "other failure" would sink the permission verdict
        // in the exact combination that most needs it — macOS Local Network
        // denied *and* Bambu Studio running — and send the user to check the
        // printer when the fix is the permission toggle.
        if let Some(failure) = &self.passive.bind_failure
            && failure.kind != "AddrInUse"
        {
            tally(failure);
        }

        denials > 0 && other_failures == 0
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    fn lan_interface(decision: InterfaceDecision) -> InterfaceReport {
        InterfaceReport {
            interface: NetInterface {
                name: "en0".to_owned(),
                ip: Ipv4Addr::new(192, 168, 0, 10),
                netmask: Ipv4Addr::new(255, 255, 255, 0),
            },
            decision,
            join: None,
            search_socket: None,
        }
    }

    fn denied() -> Result<(), IoFailure> {
        Err(IoFailure {
            kind: "PermissionDenied".to_owned(),
            message: "operation not permitted".to_owned(),
        })
    }

    #[test]
    fn finding_anything_is_the_verdict_that_matters() {
        let diagnostics = DiscoveryDiagnostics::default();
        assert_eq!(diagnostics.verdict(2), DiscoveryVerdict::Found);
    }

    #[test]
    fn no_interfaces_at_all_means_no_usable_interface() {
        let diagnostics = DiscoveryDiagnostics::default();
        assert_eq!(diagnostics.verdict(0), DiscoveryVerdict::NoUsableInterface);
    }

    #[test]
    fn only_skipped_interfaces_means_no_usable_interface() {
        let diagnostics = DiscoveryDiagnostics {
            interfaces: vec![lan_interface(InterfaceDecision::SkipLoopback)],
            ..DiscoveryDiagnostics::default()
        };
        assert_eq!(diagnostics.verdict(0), DiscoveryVerdict::NoUsableInterface);
    }

    #[test]
    fn wall_to_wall_permission_failures_are_reported_as_permission() {
        let mut report = lan_interface(InterfaceDecision::Use);
        report.join = Some(denied());
        report.search_socket = Some(denied());

        let diagnostics = DiscoveryDiagnostics {
            interfaces: vec![report],
            ..DiscoveryDiagnostics::default()
        };
        assert_eq!(diagnostics.verdict(0), DiscoveryVerdict::PermissionDenied);
    }

    #[test]
    fn macos_local_network_denial_masquerading_as_no_route_is_permission() {
        // macOS 14+ denies Local Network access by letting sockets open and
        // groups join, then failing the actual sends with "no route to
        // host" rather than EPERM. Open/join successes are local kernel
        // state — they must not launder the denial into "no response".
        let mut report = lan_interface(InterfaceDecision::Use);
        report.join = Some(Ok(()));
        report.search_socket = Some(Ok(()));

        let diagnostics = DiscoveryDiagnostics {
            interfaces: vec![report],
            active: ActiveReport {
                searches_sent: 0,
                send_failures: vec![IoFailure {
                    kind: "HostUnreachable".to_owned(),
                    message: "no route to host".to_owned(),
                }],
                ..ActiveReport::default()
            },
            ..DiscoveryDiagnostics::default()
        };
        assert_eq!(diagnostics.verdict(0), DiscoveryVerdict::PermissionDenied);
    }

    #[test]
    fn one_successful_send_downgrades_permission_to_silence() {
        // A search actually left the machine: the OS is not the obstacle,
        // whatever else failed.
        let mut report = lan_interface(InterfaceDecision::Use);
        report.join = Some(denied());

        let diagnostics = DiscoveryDiagnostics {
            interfaces: vec![report],
            active: ActiveReport {
                searches_sent: 1,
                ..ActiveReport::default()
            },
            ..DiscoveryDiagnostics::default()
        };
        assert_eq!(diagnostics.verdict(0), DiscoveryVerdict::NoResponse);
    }

    #[test]
    fn a_busy_ssdp_port_does_not_dilute_a_permission_denial() {
        // The combination that most needs the permission verdict: macOS Local
        // Network denied (every send HostUnreachable) AND Bambu Studio holding
        // port 2021 (bind AddrInUse). The bind failure is companion-app noise,
        // not counter-evidence — the verdict must still be PermissionDenied.
        let mut report = lan_interface(InterfaceDecision::Use);
        report.search_socket = Some(Ok(()));

        let diagnostics = DiscoveryDiagnostics {
            interfaces: vec![report],
            passive: PassiveReport {
                bind_failure: Some(IoFailure {
                    kind: "AddrInUse".to_owned(),
                    message: "address in use".to_owned(),
                }),
                ..PassiveReport::default()
            },
            active: ActiveReport {
                send_failures: vec![IoFailure {
                    kind: "HostUnreachable".to_owned(),
                    message: "no route to host".to_owned(),
                }],
                ..ActiveReport::default()
            },
            ..DiscoveryDiagnostics::default()
        };
        assert_eq!(diagnostics.verdict(0), DiscoveryVerdict::PermissionDenied);
    }

    #[test]
    fn a_genuinely_unexpected_failure_dilutes_the_permission_verdict() {
        // A non-AddrInUse, non-permission failure alongside a denial is mixed
        // evidence: not a clean permission story, so fall back to silence.
        let mut report = lan_interface(InterfaceDecision::Use);
        report.join = Some(denied());
        report.search_socket = Some(Err(IoFailure {
            kind: "InvalidInput".to_owned(),
            message: "something unexpected".to_owned(),
        }));

        let diagnostics = DiscoveryDiagnostics {
            interfaces: vec![report],
            ..DiscoveryDiagnostics::default()
        };
        assert_eq!(diagnostics.verdict(0), DiscoveryVerdict::NoResponse);
    }

    #[test]
    fn a_completed_tls_handshake_proves_the_os_is_not_blocking_us() {
        // SSDP looked permission-denied, but the subnet probe reached a TLS
        // peer — bidirectional traffic demonstrably works, so this is not a
        // permission problem even though that peer was not a printer.
        let mut report = lan_interface(InterfaceDecision::Use);
        report.join = Some(denied());
        report.search_socket = Some(denied());

        let diagnostics = DiscoveryDiagnostics {
            interfaces: vec![report],
            probe: Some(ProbeReport {
                probed: 253,
                tls_peers: 1,
                serials_read: 0,
                ..ProbeReport::default()
            }),
            ..DiscoveryDiagnostics::default()
        };
        assert_eq!(diagnostics.verdict(0), DiscoveryVerdict::NoResponse);
    }

    #[test]
    fn received_datagrams_alone_defeat_the_permission_verdict() {
        // A datagram arrived even though sends were denied: traffic reached us,
        // so the OS is not the wall.
        let mut report = lan_interface(InterfaceDecision::Use);
        report.join = Some(denied());
        report.search_socket = Some(denied());

        let diagnostics = DiscoveryDiagnostics {
            interfaces: vec![report],
            passive: PassiveReport {
                datagrams: 1,
                ..PassiveReport::default()
            },
            ..DiscoveryDiagnostics::default()
        };
        assert_eq!(diagnostics.verdict(0), DiscoveryVerdict::NoResponse);
    }

    #[test]
    fn every_verdict_has_a_stable_fluent_key() {
        // The UI localises by these keys; a missing or duplicated one is a
        // silently untranslated string. Pin all four and their uniqueness.
        let keys = [
            DiscoveryVerdict::Found.key(),
            DiscoveryVerdict::NoUsableInterface.key(),
            DiscoveryVerdict::PermissionDenied.key(),
            DiscoveryVerdict::NoResponse.key(),
        ];
        assert_eq!(
            keys,
            [
                "found",
                "no-usable-interface",
                "permission-denied",
                "no-response"
            ]
        );
        let unique: std::collections::BTreeSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len(), "verdict keys must be unique");
    }

    #[test]
    fn a_healthy_silent_network_is_no_response() {
        let mut report = lan_interface(InterfaceDecision::Use);
        report.join = Some(Ok(()));
        report.search_socket = Some(Ok(()));

        let diagnostics = DiscoveryDiagnostics {
            interfaces: vec![report],
            active: ActiveReport {
                searches_sent: 3,
                ..ActiveReport::default()
            },
            ..DiscoveryDiagnostics::default()
        };
        assert_eq!(diagnostics.verdict(0), DiscoveryVerdict::NoResponse);
    }

    // Fluent-key stability is covered exhaustively by
    // `every_verdict_has_a_stable_fluent_key` above.
}
