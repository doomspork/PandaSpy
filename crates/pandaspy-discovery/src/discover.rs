//! The discovery run: SSDP on every usable interface, subnet probe as the
//! fallback, diagnostics throughout.
//!
//! Generic over the transport seams ([`SsdpStack`], [`SsdpSocket`],
//! [`PortProbe`], [`InterfaceSource`]) so the whole engine runs in tests
//! against scripted fakes under a paused tokio clock — deterministic,
//! instant, no network. The real transports live in [`crate::net`].

use std::collections::BTreeMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use crate::diag::{
    DiscoveryDiagnostics, DiscoveryVerdict, InterfaceReport, IoFailure, ProbeReport,
};
use crate::interfaces::{InterfaceSource, NetInterface, assess};
use crate::probe::{PortProbe, ProbeOutcome, serial_from_cert};
use crate::ssdp::{SsdpAnnouncement, SsdpMessageKind, SsdpSocket, m_search_payload};
use crate::subnet;
use crate::types::DiscoveredPrinter;

/// When the subnet probe runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProbePolicy {
    /// Only when SSDP produced nothing — the default. The probe is the loud
    /// fallback, not the front door.
    Auto,
    /// Always, even after SSDP hits. Useful for the add-printer screen's
    /// "search harder" button.
    Always,
    /// Never. Multicast or nothing.
    Never,
}

/// Tuning for one discovery run. The defaults are the shipped behaviour.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryOptions {
    /// Total listening window. SSDP answers arrive within a second or two;
    /// the window is generous because a sleepy Wi-Fi radio is not.
    pub window: Duration,
    /// M-SEARCH transmissions per interface, spaced by `search_spacing`.
    /// Repeated because UDP drops and printers doze.
    pub searches: u32,
    pub search_spacing: Duration,
    pub probe: ProbePolicy,
    /// Upper bound on probe targets across all interfaces.
    pub probe_cap: usize,
    /// Probes in flight at once. High enough to finish a /24 quickly, low
    /// enough not to trip consumer-router SYN alarms.
    pub probe_concurrency: usize,
    /// Backstop timeout per probe target: a hung [`PortProbe`] must not stall
    /// the walk. Set above whatever the implementation budgets internally, or
    /// it preempts a handshake that was about to succeed and mislabels the
    /// result. The real probe ([`crate::net::TlsCertProbe`]) budgets 0.8s
    /// connect + 1.5s handshake, so this must exceed ~2.3s.
    pub probe_timeout: Duration,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            window: Duration::from_secs(6),
            searches: 3,
            search_spacing: Duration::from_secs(1),
            probe: ProbePolicy::Auto,
            probe_cap: 1024,
            probe_concurrency: 64,
            // 3s > the real probe's 0.8s connect + 1.5s handshake, so the
            // backstop only fires on a genuinely stuck implementation.
            probe_timeout: Duration::from_secs(3),
        }
    }
}

/// What a discovery run returns: findings plus the mechanical record.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryOutcome {
    /// Deduplicated (by serial, else address), deterministically ordered.
    pub printers: Vec<DiscoveredPrinter>,
    pub diagnostics: DiscoveryDiagnostics,
    pub verdict: DiscoveryVerdict,
}

/// The result of opening the shared passive listener.
#[derive(Debug)]
pub struct PassiveSetup<S> {
    /// `None` when binding failed entirely (see `bind_failure`).
    pub socket: Option<S>,
    pub bind_failure: Option<IoFailure>,
    /// Per-interface multicast join outcomes, keyed by the full interface —
    /// **not** by name. One OS interface can carry several IPv4 addresses (a
    /// self-assigned 169.254 alongside a DHCP lease on the same `en0` is
    /// routine on macOS), so name alone would land a join outcome on the
    /// wrong twin and flip the diagnostics verdict.
    pub joins: Vec<(NetInterface, Result<(), IoFailure>)>,
}

/// Opens sockets. The seam between the engine and the operating system.
pub trait SsdpStack: Send + Sync {
    type Socket: SsdpSocket + 'static;

    /// One listener bound to the SSDP port, joined to the multicast group on
    /// every given interface. Partial failure is normal and reported
    /// per-interface — one interface refusing must not cost the others.
    fn open_passive(
        &self,
        interfaces: &[NetInterface],
    ) -> impl Future<Output = PassiveSetup<Self::Socket>> + Send;

    /// An ephemeral-port socket that transmits M-SEARCH from this specific
    /// interface and receives its unicast replies.
    fn open_search(
        &self,
        interface: &NetInterface,
    ) -> impl Future<Output = Result<Self::Socket, IoFailure>> + Send;
}

/// Run discovery end to end.
pub async fn discover<Stack, Probe, Ifaces>(
    stack: &Stack,
    probe: Arc<Probe>,
    interface_source: &Ifaces,
    options: &DiscoveryOptions,
) -> DiscoveryOutcome
where
    Stack: SsdpStack,
    Probe: PortProbe + 'static,
    Ifaces: InterfaceSource,
{
    let mut diagnostics = DiscoveryDiagnostics::default();

    // ── 1. Interfaces ────────────────────────────────────────────────────
    let interfaces = match interface_source.interfaces() {
        Ok(interfaces) => interfaces,
        Err(error) => {
            diagnostics.enumeration_failure = Some(IoFailure::from_io(&error));
            let verdict = diagnostics.verdict(0);
            return DiscoveryOutcome {
                printers: Vec::new(),
                diagnostics,
                verdict,
            };
        }
    };

    diagnostics.interfaces = interfaces
        .iter()
        .map(|interface| InterfaceReport {
            interface: interface.clone(),
            decision: assess(interface),
            join: None,
            search_socket: None,
        })
        .collect();

    let usable: Vec<NetInterface> = interfaces
        .iter()
        .filter(|interface| assess(interface).usable())
        .cloned()
        .collect();

    // ── 2. SSDP: open sockets ────────────────────────────────────────────
    let mut tasks = tokio::task::JoinSet::new();
    let deadline = Instant::now() + options.window;

    if !usable.is_empty() {
        let setup = stack.open_passive(&usable).await;
        diagnostics.passive.bind_failure = setup.bind_failure;
        for (interface, join) in setup.joins {
            if let Some(report) = diagnostics
                .interfaces
                .iter_mut()
                .find(|report| report.interface == interface)
            {
                report.join = Some(join);
            }
        }
        if let Some(socket) = setup.socket {
            tasks.spawn(run_socket(
                socket,
                SocketRole::Passive,
                options.clone(),
                deadline,
            ));
        }

        for interface in &usable {
            match stack.open_search(interface).await {
                Ok(socket) => {
                    mark_search(&mut diagnostics, interface, Ok(()));
                    tasks.spawn(run_socket(
                        socket,
                        SocketRole::Search,
                        options.clone(),
                        deadline,
                    ));
                }
                Err(failure) => {
                    mark_search(&mut diagnostics, interface, Err(failure));
                }
            }
        }
    }

    // ── 3. SSDP: listen, then fold datagrams into printers ──────────────
    let mut printers: BTreeMap<String, DiscoveredPrinter> = BTreeMap::new();

    while let Some(joined) = tasks.join_next().await {
        // A panicked socket task loses that socket's datagrams, not the run.
        let Ok(log) = joined else { continue };

        match log.role {
            SocketRole::Passive => {
                diagnostics.passive.datagrams += log.datagrams.len() as u32;
                diagnostics.passive.recv_failures.extend(log.recv_failures);
            }
            SocketRole::Search => {
                diagnostics.active.datagrams += log.datagrams.len() as u32;
                diagnostics.active.searches_sent += log.sent;
                diagnostics.active.recv_failures.extend(log.recv_failures);
            }
        }
        diagnostics.active.send_failures.extend(log.send_failures);

        for (bytes, sender) in log.datagrams {
            match SsdpAnnouncement::parse(&bytes) {
                Some(announcement) if announcement.is_announcement() => {
                    match announcement.to_printer(sender) {
                        Some(printer) => {
                            diagnostics.parse.announcements += 1;
                            merge(&mut printers, printer);
                        }
                        None => diagnostics.parse.ignored += 1,
                    }
                }
                Some(announcement) if announcement.kind == SsdpMessageKind::MSearch => {
                    diagnostics.parse.msearch_echoes += 1;
                }
                _ => diagnostics.parse.ignored += 1,
            }
        }
    }

    // ── 4. Subnet probe fallback ─────────────────────────────────────────
    let run_probe = match options.probe {
        ProbePolicy::Always => true,
        ProbePolicy::Auto => printers.is_empty(),
        ProbePolicy::Never => false,
    };

    if run_probe && !usable.is_empty() {
        let plan = subnet::plan(&usable, options.probe_cap);
        let mut report = ProbeReport {
            planned: plan.targets.len(),
            clamped: plan.clamped.clone(),
            truncated: plan.truncated,
            ..ProbeReport::default()
        };

        // At least one in flight: a configured concurrency of 0 would spawn
        // nothing, join nothing, and silently skip the whole probe.
        let concurrency = options.probe_concurrency.max(1);
        let mut probes = tokio::task::JoinSet::new();
        let mut targets = plan.targets.into_iter();
        loop {
            while probes.len() < concurrency {
                let Some(ip) = targets.next() else { break };
                let target = SocketAddr::from((ip, pandaspy_proto::wire::MQTT_PORT));
                let probe = Arc::clone(&probe);
                let backstop = options.probe_timeout;
                probes.spawn(async move {
                    let outcome = tokio::time::timeout(backstop, probe.probe(target))
                        .await
                        .unwrap_or(ProbeOutcome::Timeout);
                    (target, outcome)
                });
            }

            let Some(joined) = probes.join_next().await else {
                break; // no tasks left and no targets left
            };
            report.probed += 1;
            let Ok((target, outcome)) = joined else {
                continue;
            };
            if let ProbeOutcome::TlsPeer { cert_der } = outcome {
                report.tls_peers += 1;
                if let Some(serial) = serial_from_cert(&cert_der) {
                    report.serials_read += 1;
                    merge(
                        &mut printers,
                        DiscoveredPrinter::from_probe(target.ip(), serial),
                    );
                }
            }
        }

        diagnostics.probe = Some(report);
    }

    // ── 5. Verdict ───────────────────────────────────────────────────────
    let printers: Vec<DiscoveredPrinter> = printers.into_values().collect();
    let verdict = diagnostics.verdict(printers.len());
    DiscoveryOutcome {
        printers,
        diagnostics,
        verdict,
    }
}

fn mark_search(
    diagnostics: &mut DiscoveryDiagnostics,
    interface: &NetInterface,
    outcome: Result<(), IoFailure>,
) {
    // Match on full interface identity, not name — see `PassiveSetup::joins`.
    if let Some(report) = diagnostics
        .interfaces
        .iter_mut()
        .find(|report| &report.interface == interface)
    {
        report.search_socket = Some(outcome);
    }
}

fn merge(printers: &mut BTreeMap<String, DiscoveredPrinter>, printer: DiscoveredPrinter) {
    match printers.entry(printer.dedup_key()) {
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            entry.get_mut().absorb(&printer);
        }
        std::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert(printer);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocketRole {
    Passive,
    Search,
}

struct SocketLog {
    role: SocketRole,
    sent: u32,
    send_failures: Vec<IoFailure>,
    /// recv errors that did not end the socket. On Windows a UDP socket whose
    /// earlier datagram drew an ICMP "port unreachable" reports
    /// `ConnectionReset` on its *next* recv — a per-datagram artefact, not a
    /// dead socket. Kept for diagnostics; see the loop below.
    recv_failures: Vec<IoFailure>,
    datagrams: Vec<(Vec<u8>, SocketAddr)>,
}

/// Drive one socket until the deadline: transmit M-SEARCHes on schedule
/// (search sockets only) while collecting every datagram that arrives.
async fn run_socket<S: SsdpSocket>(
    socket: S,
    role: SocketRole,
    options: DiscoveryOptions,
    deadline: Instant,
) -> SocketLog {
    /// Per-socket memory bound; a multicast storm must not become ours.
    const MAX_DATAGRAMS: usize = 256;
    /// A recv that fails this many times in a row with nothing in between is
    /// treated as a genuinely broken socket rather than transient noise, and
    /// ends the listen loop. High enough that the Windows ICMP-reset artefact
    /// (one error per unreachable peer we searched) never trips it; low enough
    /// that a hard-down socket cannot spin for the whole window.
    const MAX_CONSECUTIVE_RECV_ERRORS: u32 = 16;

    let mut log = SocketLog {
        role,
        sent: 0,
        send_failures: Vec::new(),
        recv_failures: Vec::new(),
        datagrams: Vec::new(),
    };
    let payload = m_search_payload();
    let planned_sends = if role == SocketRole::Search {
        options.searches
    } else {
        0
    };

    let mut send_timer = Box::pin(tokio::time::sleep(Duration::ZERO));
    let mut buffer = vec![0_u8; 2048];
    let mut consecutive_recv_errors = 0_u32;

    loop {
        tokio::select! {
            () = tokio::time::sleep_until(deadline) => break,

            () = &mut send_timer, if log.sent + (log.send_failures.len() as u32) < planned_sends => {
                // Bound the send by the time left in the window. Awaiting it
                // bare would let a send that hangs (a wedged interface) stall
                // this task — and therefore the whole run — past the deadline,
                // because the deadline branch cannot be reached while we are
                // parked inside the handler body.
                let remaining = deadline.saturating_duration_since(Instant::now());
                match tokio::time::timeout(remaining, socket.send_search(&payload)).await {
                    Ok(Ok(())) => log.sent += 1,
                    Ok(Err(error)) => log.send_failures.push(IoFailure::from_io(&error)),
                    Err(_elapsed) => break, // deadline reached mid-send
                }
                send_timer = Box::pin(tokio::time::sleep(options.search_spacing));
            }

            received = socket.recv(&mut buffer) => {
                match received {
                    Ok((length, sender)) => {
                        consecutive_recv_errors = 0;
                        if log.datagrams.len() < MAX_DATAGRAMS {
                            log.datagrams.push((buffer[..length].to_vec(), sender));
                        }
                    }
                    // A single recv error is usually transient — most often the
                    // Windows ICMP-reset artefact after we multicast an
                    // M-SEARCH that some host answered with "unreachable".
                    // Treating it as terminal (the old behaviour) left that
                    // interface deaf for the rest of the window, so a printer
                    // replying 200ms later was never heard. Count and carry on;
                    // give up only if a socket fails repeatedly with no
                    // intervening success, so a genuinely dead socket cannot
                    // spin.
                    Err(error) => {
                        log.recv_failures.push(IoFailure::from_io(&error));
                        consecutive_recv_errors += 1;
                        if consecutive_recv_errors >= MAX_CONSECUTIVE_RECV_ERRORS {
                            break;
                        }
                    }
                }
            }
        }
    }

    log
}
