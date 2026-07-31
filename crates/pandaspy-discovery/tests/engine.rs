//! The discovery engine, driven end to end by scripted fakes under a paused
//! tokio clock. No network is touched; every test is deterministic and
//! instant — which is the entire argument for the trait seam.

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pandaspy_discovery::{
    DiscoveryOptions, DiscoveryOutcome, DiscoverySource, DiscoveryVerdict, InterfaceSource,
    IoFailure, NetInterface, PassiveSetup, PortProbe, ProbeOutcome, ProbePolicy, SsdpSocket,
    SsdpStack,
};
use tokio::sync::Mutex;

// ─── Fakes ───────────────────────────────────────────────────────────────

/// Something a socket's `recv` produces `after` some time on the fake clock:
/// either a datagram, or an error (to model transient recv failures like the
/// Windows ICMP-reset artefact).
struct Scripted {
    after: Duration,
    outcome: Result<(Vec<u8>, SocketAddr), io::ErrorKind>,
}

impl Scripted {
    fn datagram(after: Duration, bytes: Vec<u8>, from: SocketAddr) -> Self {
        Self {
            after,
            outcome: Ok((bytes, from)),
        }
    }

    fn error(after: Duration, kind: io::ErrorKind) -> Self {
        Self {
            after,
            outcome: Err(kind),
        }
    }
}

#[derive(Debug, Default)]
struct FakeSocket {
    script: Mutex<Vec<Scripted>>,
    send_error: Option<io::ErrorKind>,
    sends: AtomicUsize,
}

impl std::fmt::Debug for Scripted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Scripted(after {:?})", self.after)
    }
}

impl FakeSocket {
    fn with_script(script: Vec<Scripted>) -> Self {
        Self {
            script: Mutex::new(script),
            ..Self::default()
        }
    }

    fn failing_sends(kind: io::ErrorKind) -> Self {
        Self {
            send_error: Some(kind),
            ..Self::default()
        }
    }
}

impl SsdpSocket for FakeSocket {
    async fn send_search(&self, _payload: &[u8]) -> io::Result<()> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        match self.send_error {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(()),
        }
    }

    async fn recv(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        // Cancel-safety matters even for a fake: the engine's `select!`
        // drops this future whenever another branch wins. Sleep BEFORE
        // popping, so a cancelled recv never loses a scripted datagram.
        let delay = match self.script.lock().await.first() {
            Some(item) => item.after,
            None => {
                // Nothing more will ever arrive: park forever and let the
                // engine's deadline win the select.
                std::future::pending::<Duration>().await
            }
        };
        tokio::time::sleep(delay).await;

        let item = self.script.lock().await.remove(0);
        match item.outcome {
            Ok((bytes, from)) => {
                buf[..bytes.len()].copy_from_slice(&bytes);
                Ok((bytes.len(), from))
            }
            Err(kind) => Err(io::Error::from(kind)),
        }
    }
}

#[derive(Debug)]
struct FakeStack {
    passive: Mutex<Option<PassiveSetup<FakeSocket>>>,
    search: Mutex<HashMap<String, Result<FakeSocket, IoFailure>>>,
}

impl FakeStack {
    fn new(
        passive: PassiveSetup<FakeSocket>,
        search: Vec<(&str, Result<FakeSocket, IoFailure>)>,
    ) -> Self {
        Self {
            passive: Mutex::new(Some(passive)),
            search: Mutex::new(
                search
                    .into_iter()
                    .map(|(name, socket)| (name.to_owned(), socket))
                    .collect(),
            ),
        }
    }
}

impl SsdpStack for FakeStack {
    type Socket = FakeSocket;

    async fn open_passive(&self, _interfaces: &[NetInterface]) -> PassiveSetup<FakeSocket> {
        self.passive.lock().await.take().unwrap_or(PassiveSetup {
            socket: None,
            bind_failure: None,
            joins: Vec::new(),
        })
    }

    async fn open_search(&self, interface: &NetInterface) -> Result<FakeSocket, IoFailure> {
        self.search
            .lock()
            .await
            .remove(&interface.name)
            .unwrap_or_else(|| Ok(FakeSocket::default()))
    }
}

#[derive(Debug, Default)]
struct FakeProbe {
    outcomes: HashMap<SocketAddr, ProbeOutcome>,
    probes: AtomicUsize,
    /// Peak number of probes seen in flight at once — how the concurrency
    /// ceiling is asserted.
    in_flight: Arc<AtomicUsize>,
    max_in_flight: Arc<AtomicUsize>,
    /// When set, every probe parks for this long. Longer than the engine's
    /// backstop, this models a hung `PortProbe` the backstop must rescue.
    stall: Option<Duration>,
}

impl PortProbe for FakeProbe {
    async fn probe(&self, target: SocketAddr) -> ProbeOutcome {
        self.probes.fetch_add(1, Ordering::SeqCst);
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(now, Ordering::SeqCst);

        if let Some(stall) = self.stall {
            tokio::time::sleep(stall).await;
        }

        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        self.outcomes
            .get(&target)
            .cloned()
            .unwrap_or(ProbeOutcome::Refused)
    }
}

#[derive(Debug)]
struct FakeInterfaces(io::Result<Vec<NetInterface>>);

impl InterfaceSource for FakeInterfaces {
    fn interfaces(&self) -> io::Result<Vec<NetInterface>> {
        match &self.0 {
            Ok(list) => Ok(list.clone()),
            Err(error) => Err(io::Error::from(error.kind())),
        }
    }
}

// ─── Scenario helpers ────────────────────────────────────────────────────

fn lan(name: &str, last_octet: u8) -> NetInterface {
    NetInterface {
        name: name.to_owned(),
        ip: Ipv4Addr::new(192, 168, 0, last_octet),
        netmask: Ipv4Addr::new(255, 255, 255, 0),
    }
}

fn notify_datagram(serial: &str, ip: &str) -> Vec<u8> {
    format!(
        "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:2021\r\nLocation: {ip}\r\n\
         USN: {serial}\r\nDevModel.bambu.com: C12\r\nDevName.bambu.com: bench\r\n\r\n"
    )
    .into_bytes()
}

/// The same announcement as an M-SEARCH reply (`HTTP/1.1 200 OK`) rather than
/// an unsolicited NOTIFY.
fn search_response(serial: &str, ip: &str) -> Vec<u8> {
    let text = String::from_utf8(notify_datagram(serial, ip)).unwrap();
    text.replace("NOTIFY * HTTP/1.1", "HTTP/1.1 200 OK")
        .into_bytes()
}

fn from_printer(ip: &str) -> SocketAddr {
    format!("{ip}:2021").parse().unwrap()
}

fn joins_ok(interfaces: &[NetInterface]) -> Vec<(NetInterface, Result<(), IoFailure>)> {
    interfaces.iter().map(|i| (i.clone(), Ok(()))).collect()
}

fn denied(kind: &str) -> IoFailure {
    IoFailure {
        kind: kind.to_owned(),
        message: "denied by test".to_owned(),
    }
}

fn options() -> DiscoveryOptions {
    DiscoveryOptions {
        probe: ProbePolicy::Auto,
        ..DiscoveryOptions::default()
    }
}

async fn run(
    stack: FakeStack,
    probe: FakeProbe,
    interfaces: FakeInterfaces,
    options: DiscoveryOptions,
) -> DiscoveryOutcome {
    pandaspy_discovery::discover(&stack, Arc::new(probe), &interfaces, &options).await
}

// ─── The scenarios ───────────────────────────────────────────────────────

#[tokio::test(start_paused = true)]
async fn a_passive_notify_finds_a_printer() {
    let passive = PassiveSetup {
        socket: Some(FakeSocket::with_script(vec![Scripted::datagram(
            Duration::from_millis(300),
            notify_datagram("00M09A000000000", "192.168.0.30"),
            from_printer("192.168.0.30"),
        )])),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10)]),
    };
    let stack = FakeStack::new(passive, vec![]);

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        options(),
    )
    .await;

    assert_eq!(outcome.verdict, DiscoveryVerdict::Found);
    assert_eq!(outcome.printers.len(), 1);
    let printer = &outcome.printers[0];
    assert_eq!(printer.serial.as_ref().unwrap().0, "00M09A000000000");
    assert_eq!(printer.source, DiscoverySource::Ssdp);
    assert_eq!(outcome.diagnostics.passive.datagrams, 1);
    assert_eq!(outcome.diagnostics.parse.announcements, 1);
    assert!(
        outcome.diagnostics.probe.is_none(),
        "SSDP succeeded; the loud fallback must not have run"
    );
}

#[tokio::test(start_paused = true)]
async fn search_responses_count_and_searches_are_paced() {
    let search_socket = FakeSocket::with_script(vec![Scripted::datagram(
        Duration::from_millis(1500),
        search_response("00M09A000000000", "192.168.0.30"),
        from_printer("192.168.0.30"),
    )]);

    let passive = PassiveSetup {
        socket: Some(FakeSocket::default()),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10)]),
    };
    let stack = FakeStack::new(passive, vec![("en0", Ok(search_socket))]);

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        options(),
    )
    .await;

    assert_eq!(outcome.verdict, DiscoveryVerdict::Found);
    assert_eq!(outcome.diagnostics.active.datagrams, 1);
    assert_eq!(
        outcome.diagnostics.active.searches_sent, 3,
        "all three M-SEARCHes fit inside the window"
    );
}

#[tokio::test(start_paused = true)]
async fn the_same_printer_on_two_interfaces_is_one_printer() {
    let datagram = |ip: &str| {
        Scripted::datagram(
            Duration::from_millis(100),
            notify_datagram("00M09A000000000", "192.168.0.30"),
            from_printer(ip),
        )
    };
    let passive = PassiveSetup {
        socket: Some(FakeSocket::with_script(vec![
            datagram("192.168.0.30"),
            datagram("192.168.0.30"),
        ])),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10), lan("en1", 11)]),
    };
    let stack = FakeStack::new(passive, vec![]);

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![lan("en0", 10), lan("en1", 11)])),
        options(),
    )
    .await;

    assert_eq!(outcome.printers.len(), 1, "deduplicated by serial");
    assert_eq!(outcome.diagnostics.parse.announcements, 2);
}

#[tokio::test(start_paused = true)]
async fn no_interfaces_is_its_own_verdict_and_opens_nothing() {
    let stack = FakeStack::new(
        PassiveSetup {
            socket: Some(FakeSocket::default()),
            bind_failure: None,
            joins: vec![],
        },
        vec![],
    );

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![])),
        options(),
    )
    .await;

    assert_eq!(outcome.verdict, DiscoveryVerdict::NoUsableInterface);
    assert_eq!(outcome.diagnostics.passive.datagrams, 0);
    assert!(outcome.diagnostics.probe.is_none(), "nothing to probe from");
}

#[tokio::test(start_paused = true)]
async fn loopback_only_machines_get_no_usable_interface_with_reasons() {
    let loopback = NetInterface {
        name: "lo0".to_owned(),
        ip: Ipv4Addr::LOCALHOST,
        netmask: Ipv4Addr::new(255, 0, 0, 0),
    };
    let stack = FakeStack::new(
        PassiveSetup {
            socket: None,
            bind_failure: None,
            joins: vec![],
        },
        vec![],
    );

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![loopback])),
        options(),
    )
    .await;

    assert_eq!(outcome.verdict, DiscoveryVerdict::NoUsableInterface);
    assert_eq!(
        outcome.diagnostics.interfaces.len(),
        1,
        "skips are recorded"
    );
}

#[tokio::test(start_paused = true)]
async fn interface_enumeration_failure_is_reported_not_swallowed() {
    let stack = FakeStack::new(
        PassiveSetup {
            socket: None,
            bind_failure: None,
            joins: vec![],
        },
        vec![],
    );

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Err(io::Error::from(io::ErrorKind::PermissionDenied))),
        options(),
    )
    .await;

    assert_eq!(outcome.verdict, DiscoveryVerdict::NoUsableInterface);
    assert!(outcome.diagnostics.enumeration_failure.is_some());
}

#[tokio::test(start_paused = true)]
async fn permission_denials_across_the_board_get_the_permission_verdict() {
    let passive = PassiveSetup {
        socket: None,
        bind_failure: Some(denied("PermissionDenied")),
        joins: vec![(lan("en0", 10), Err(denied("PermissionDenied")))],
    };
    let stack = FakeStack::new(passive, vec![("en0", Err(denied("PermissionDenied")))]);

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        DiscoveryOptions {
            probe: ProbePolicy::Never,
            ..options()
        },
    )
    .await;

    assert_eq!(outcome.verdict, DiscoveryVerdict::PermissionDenied);
}

#[tokio::test(start_paused = true)]
async fn macos_style_no_route_multicast_sends_read_as_permission() {
    // Local Network permission denied on macOS: sockets open, joins "work",
    // and every multicast send dies with HostUnreachable.
    let passive = PassiveSetup {
        socket: None,
        bind_failure: Some(denied("HostUnreachable")),
        joins: vec![(lan("en0", 10), Err(denied("HostUnreachable")))],
    };
    let stack = FakeStack::new(
        passive,
        vec![(
            "en0",
            Ok(FakeSocket::failing_sends(io::ErrorKind::HostUnreachable)),
        )],
    );

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        DiscoveryOptions {
            probe: ProbePolicy::Never,
            ..options()
        },
    )
    .await;

    assert_eq!(outcome.verdict, DiscoveryVerdict::PermissionDenied);
    assert!(!outcome.diagnostics.active.send_failures.is_empty());
}

#[tokio::test(start_paused = true)]
async fn silence_with_working_sockets_is_no_response() {
    let passive = PassiveSetup {
        socket: Some(FakeSocket::default()),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10)]),
    };
    let stack = FakeStack::new(passive, vec![("en0", Ok(FakeSocket::default()))]);

    let outcome = run(
        stack,
        FakeProbe::default(), // every probe answers Refused
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        options(),
    )
    .await;

    assert_eq!(outcome.verdict, DiscoveryVerdict::NoResponse);
    let probe = outcome
        .diagnostics
        .probe
        .expect("auto probe ran on silence");
    assert_eq!(probe.planned, 253);
    assert_eq!(probe.probed, 253);
    assert_eq!(probe.tls_peers, 0);
}

#[tokio::test(start_paused = true)]
async fn the_subnet_probe_rescues_a_multicast_dead_network() {
    let printer_addr: SocketAddr = "192.168.0.77:8883".parse().unwrap();

    // Mint a real self-signed certificate whose CN is the serial — exactly
    // the evidence a live printer presents.
    let mut params = rcgen::CertificateParams::default();
    params.distinguished_name = rcgen::DistinguishedName::new();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "01P00A000000000");
    let key = rcgen::KeyPair::generate().unwrap();
    let cert_der = params.self_signed(&key).unwrap().der().to_vec();

    let mut probe = FakeProbe::default();
    probe
        .outcomes
        .insert(printer_addr, ProbeOutcome::TlsPeer { cert_der });

    let passive = PassiveSetup {
        socket: Some(FakeSocket::default()),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10)]),
    };
    let stack = FakeStack::new(passive, vec![("en0", Ok(FakeSocket::default()))]);

    let outcome = run(
        stack,
        probe,
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        options(),
    )
    .await;

    assert_eq!(outcome.verdict, DiscoveryVerdict::Found);
    assert_eq!(outcome.printers.len(), 1);
    let printer = &outcome.printers[0];
    assert_eq!(printer.serial.as_ref().unwrap().0, "01P00A000000000");
    assert_eq!(printer.source, DiscoverySource::SubnetProbe);
    assert_eq!(printer.address, printer_addr.ip());

    let report = outcome.diagnostics.probe.unwrap();
    assert_eq!(report.tls_peers, 1);
    assert_eq!(report.serials_read, 1);
}

#[tokio::test(start_paused = true)]
async fn probe_policy_never_means_never() {
    let passive = PassiveSetup {
        socket: Some(FakeSocket::default()),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10)]),
    };
    let stack = FakeStack::new(passive, vec![("en0", Ok(FakeSocket::default()))]);
    let probe = FakeProbe::default();

    let outcome = run(
        stack,
        probe,
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        DiscoveryOptions {
            probe: ProbePolicy::Never,
            ..options()
        },
    )
    .await;

    assert_eq!(outcome.verdict, DiscoveryVerdict::NoResponse);
    assert!(outcome.diagnostics.probe.is_none());
}

#[tokio::test(start_paused = true)]
async fn a_busy_ssdp_port_downgrades_gracefully_to_active_search() {
    // Bambu Studio holds 2021. Passive bind fails; the search socket still
    // finds the printer; the failure is recorded as context.
    let search_socket = FakeSocket::with_script(vec![Scripted::datagram(
        Duration::from_millis(200),
        search_response("00M09A000000000", "192.168.0.30"),
        from_printer("192.168.0.30"),
    )]);

    let passive = PassiveSetup {
        socket: None,
        bind_failure: Some(denied("AddrInUse")),
        joins: vec![],
    };
    let stack = FakeStack::new(passive, vec![("en0", Ok(search_socket))]);

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        options(),
    )
    .await;

    assert_eq!(outcome.verdict, DiscoveryVerdict::Found);
    assert_eq!(
        outcome
            .diagnostics
            .passive
            .bind_failure
            .as_ref()
            .unwrap()
            .kind,
        "AddrInUse"
    );
}

#[tokio::test(start_paused = true)]
async fn msearch_echoes_and_garbage_are_counted_not_confused() {
    let passive = PassiveSetup {
        socket: Some(FakeSocket::with_script(vec![
            Scripted::datagram(
                Duration::from_millis(10),
                pandaspy_discovery::ssdp::m_search_payload(),
                "192.168.0.10:52000".parse().unwrap(),
            ),
            Scripted::datagram(
                Duration::from_millis(20),
                vec![0x00, 0xde, 0xad],
                "192.168.0.66:1900".parse().unwrap(),
            ),
        ])),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10)]),
    };
    let stack = FakeStack::new(passive, vec![]);

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        DiscoveryOptions {
            probe: ProbePolicy::Never,
            ..options()
        },
    )
    .await;

    assert!(outcome.printers.is_empty());
    assert_eq!(outcome.diagnostics.parse.msearch_echoes, 1);
    assert_eq!(outcome.diagnostics.parse.ignored, 1);
    assert_eq!(outcome.diagnostics.parse.announcements, 0);
}

/// A self-signed certificate whose CN is `serial` — what a printer serves and
/// what the probe reads back.
fn printer_cert(serial: &str) -> Vec<u8> {
    let mut params = rcgen::CertificateParams::default();
    params.distinguished_name = rcgen::DistinguishedName::new();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, serial);
    let key = rcgen::KeyPair::generate().unwrap();
    params.self_signed(&key).unwrap().der().to_vec()
}

#[tokio::test(start_paused = true)]
async fn mixed_interface_success_and_failure_lands_on_the_right_reports() {
    // The crate's core multi-NIC promise: one interface refusing must not cost
    // the others, and each outcome must be recorded against its own interface.
    // en0 works (a printer answers); en1 is denied on both join and search.
    let en0 = lan("en0", 10);
    let en1 = lan("en1", 11);

    let passive = PassiveSetup {
        socket: Some(FakeSocket::with_script(vec![Scripted::datagram(
            Duration::from_millis(100),
            notify_datagram("00M09A000000000", "192.168.0.30"),
            from_printer("192.168.0.30"),
        )])),
        bind_failure: None,
        joins: vec![
            (en0.clone(), Ok(())),
            (en1.clone(), Err(denied("PermissionDenied"))),
        ],
    };
    let stack = FakeStack::new(
        passive,
        vec![
            ("en0", Ok(FakeSocket::default())),
            ("en1", Err(denied("PermissionDenied"))),
        ],
    );

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![en0.clone(), en1.clone()])),
        options(),
    )
    .await;

    assert_eq!(
        outcome.verdict,
        DiscoveryVerdict::Found,
        "en0's printer wins"
    );
    assert_eq!(outcome.printers.len(), 1);

    let report = |iface: &NetInterface| {
        outcome
            .diagnostics
            .interfaces
            .iter()
            .find(|r| &r.interface == iface)
            .unwrap()
            .clone()
    };
    assert_eq!(report(&en0).join, Some(Ok(())), "en0 join recorded on en0");
    assert!(
        matches!(report(&en1).join, Some(Err(_))),
        "en1's denial must land on en1, not en0"
    );
    assert!(matches!(report(&en1).search_socket, Some(Err(_))));
}

#[tokio::test(start_paused = true)]
async fn interface_aliases_do_not_cross_wire_their_outcomes() {
    // One OS interface, two IPv4 addresses (a self-assigned link-local twin
    // beside a real DHCP lease — routine on macOS). The denial recorded for
    // the usable address must land on the usable report; name-matching alone
    // would put it on the link-local twin and mislabel the verdict.
    let link_local = NetInterface {
        name: "en0".to_owned(),
        ip: Ipv4Addr::new(169, 254, 12, 7),
        netmask: Ipv4Addr::new(255, 255, 0, 0),
    };
    let usable = lan("en0", 10); // same name, different address

    let passive = PassiveSetup {
        socket: None,
        bind_failure: None,
        // open_passive only runs for the usable address.
        joins: vec![(usable.clone(), Err(denied("PermissionDenied")))],
    };
    let stack = FakeStack::new(passive, vec![("en0", Err(denied("PermissionDenied")))]);

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![link_local.clone(), usable.clone()])),
        DiscoveryOptions {
            probe: ProbePolicy::Never,
            ..options()
        },
    )
    .await;

    assert_eq!(
        outcome.verdict,
        DiscoveryVerdict::PermissionDenied,
        "the denial on the usable alias must drive the verdict"
    );
    let usable_report = outcome
        .diagnostics
        .interfaces
        .iter()
        .find(|r| r.interface == usable)
        .unwrap();
    assert!(matches!(usable_report.join, Some(Err(_))));
}

#[tokio::test(start_paused = true)]
async fn probe_policy_always_runs_the_probe_and_merges_across_sources() {
    // The 'search harder' button: even though SSDP already found the printer,
    // the probe runs, and a probe hit for the same serial collapses into the
    // SSDP sighting rather than doubling it.
    let mut probe = FakeProbe::default();
    probe.outcomes.insert(
        "192.168.0.30:8883".parse().unwrap(),
        ProbeOutcome::TlsPeer {
            cert_der: printer_cert("00M09A000000000"),
        },
    );

    let passive = PassiveSetup {
        socket: Some(FakeSocket::with_script(vec![Scripted::datagram(
            Duration::from_millis(50),
            notify_datagram("00M09A000000000", "192.168.0.30"),
            from_printer("192.168.0.30"),
        )])),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10)]),
    };
    let stack = FakeStack::new(passive, vec![]);

    let outcome = run(
        stack,
        probe,
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        DiscoveryOptions {
            probe: ProbePolicy::Always,
            ..options()
        },
    )
    .await;

    assert!(
        outcome.diagnostics.probe.is_some(),
        "Always runs the probe even after SSDP succeeds"
    );
    assert_eq!(
        outcome.printers.len(),
        1,
        "same serial merges to one printer"
    );
    assert_eq!(
        outcome.printers[0].source,
        DiscoverySource::Ssdp,
        "the richer SSDP sighting wins the merge"
    );
}

#[tokio::test(start_paused = true)]
async fn a_serialless_ssdp_hit_and_a_probe_hit_at_one_address_stay_distinct() {
    // KNOWN LIMITATION, pinned so a future change to dedup is a conscious one:
    // an SSDP NOTIFY with no USN keys by address ("ip:…"), while the probe hit
    // at that same address keys by the serial it read ("sn:…"), so the two are
    // reported as two printers for one physical device.
    // TODO(fixture): if real captures show this pairing in the wild, teach
    // `merge` to reconcile an address-keyed sighting with a serial-keyed one
    // at the same IP.
    let notify_no_usn = b"NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:2021\r\n\
        Location: 192.168.0.30\r\nDevModel.bambu.com: C12\r\n\r\n"
        .to_vec();

    let mut probe = FakeProbe::default();
    probe.outcomes.insert(
        "192.168.0.30:8883".parse().unwrap(),
        ProbeOutcome::TlsPeer {
            cert_der: printer_cert("00M09A000000000"),
        },
    );

    let passive = PassiveSetup {
        socket: Some(FakeSocket::with_script(vec![Scripted::datagram(
            Duration::from_millis(50),
            notify_no_usn,
            from_printer("192.168.0.30"),
        )])),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10)]),
    };
    let stack = FakeStack::new(passive, vec![]);

    let outcome = run(
        stack,
        probe,
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        DiscoveryOptions {
            probe: ProbePolicy::Always,
            ..options()
        },
    )
    .await;

    assert_eq!(
        outcome.printers.len(),
        2,
        "known dedup limitation, see comment"
    );
}

#[tokio::test(start_paused = true)]
async fn a_hung_probe_is_rescued_by_the_backstop() {
    // A PortProbe that never returns must not stall the whole run: the
    // per-target backstop turns it into a Timeout and the walk finishes.
    let probe = FakeProbe {
        stall: Some(Duration::from_secs(60)), // >> the 3s backstop
        ..FakeProbe::default()
    };

    let passive = PassiveSetup {
        socket: Some(FakeSocket::default()),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10)]),
    };
    let stack = FakeStack::new(passive, vec![("en0", Ok(FakeSocket::default()))]);

    let outcome = run(
        stack,
        probe,
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        // Small subnet so the run is quick even one-backstop-at-a-time.
        DiscoveryOptions {
            probe: ProbePolicy::Auto,
            probe_cap: 4,
            probe_concurrency: 4,
            ..options()
        },
    )
    .await;

    // The point is that we got here at all rather than hanging forever.
    let report = outcome.diagnostics.probe.expect("probe ran");
    assert_eq!(report.probed, report.planned);
    assert_eq!(
        report.tls_peers, 0,
        "every hung probe backstopped to Timeout"
    );
}

#[tokio::test(start_paused = true)]
async fn probe_concurrency_ceiling_is_respected() {
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let probe = FakeProbe {
        stall: Some(Duration::from_millis(50)), // hold each open so they overlap
        in_flight: Arc::clone(&in_flight),
        max_in_flight: Arc::clone(&max_in_flight),
        ..FakeProbe::default()
    };

    let passive = PassiveSetup {
        socket: Some(FakeSocket::default()),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10)]),
    };
    let stack = FakeStack::new(passive, vec![("en0", Ok(FakeSocket::default()))]);

    let _ = run(
        stack,
        probe,
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        DiscoveryOptions {
            probe: ProbePolicy::Auto,
            probe_concurrency: 8,
            probe_timeout: Duration::from_secs(30), // don't backstop the stall
            ..options()
        },
    )
    .await;

    let peak = max_in_flight.load(Ordering::SeqCst);
    assert!(
        peak > 1,
        "the walk should actually run in parallel (peak {peak})"
    );
    assert!(
        peak <= 8,
        "never more than probe_concurrency in flight (peak {peak})"
    );
}

#[tokio::test(start_paused = true)]
async fn a_transient_recv_error_is_survived_and_counted() {
    // Windows surfaces ICMP port-unreachable as a ConnectionReset on the next
    // recv. The old code treated any recv error as terminal and went deaf for
    // the rest of the window; the fix counts it and keeps listening, so a
    // printer replying afterwards is still heard.
    let passive = PassiveSetup {
        socket: Some(FakeSocket::with_script(vec![
            Scripted::error(Duration::from_millis(20), io::ErrorKind::ConnectionReset),
            Scripted::datagram(
                Duration::from_millis(40),
                notify_datagram("00M09A000000000", "192.168.0.30"),
                from_printer("192.168.0.30"),
            ),
        ])),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10)]),
    };
    let stack = FakeStack::new(passive, vec![]);

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        DiscoveryOptions {
            probe: ProbePolicy::Never,
            ..options()
        },
    )
    .await;

    assert_eq!(
        outcome.verdict,
        DiscoveryVerdict::Found,
        "the datagram after the recv error is still heard"
    );
    assert_eq!(
        outcome.diagnostics.passive.recv_failures.len(),
        1,
        "the transient error is recorded, not swallowed"
    );
}

#[tokio::test(start_paused = true)]
async fn a_datagram_storm_is_capped() {
    // A flood must not grow the log without bound. Script well past the cap
    // and assert the recorded count is bounded while a printer is still found.
    let mut script = Vec::new();
    for _ in 0..300 {
        script.push(Scripted::datagram(
            Duration::ZERO,
            notify_datagram("00M09A000000000", "192.168.0.30"),
            from_printer("192.168.0.30"),
        ));
    }
    let passive = PassiveSetup {
        socket: Some(FakeSocket::with_script(script)),
        bind_failure: None,
        joins: joins_ok(&[lan("en0", 10)]),
    };
    let stack = FakeStack::new(passive, vec![]);

    let outcome = run(
        stack,
        FakeProbe::default(),
        FakeInterfaces(Ok(vec![lan("en0", 10)])),
        DiscoveryOptions {
            probe: ProbePolicy::Never,
            ..options()
        },
    )
    .await;

    assert_eq!(outcome.printers.len(), 1);
    assert!(
        outcome.diagnostics.passive.datagrams <= 256,
        "datagram log is capped, got {}",
        outcome.diagnostics.passive.datagrams
    );
}
