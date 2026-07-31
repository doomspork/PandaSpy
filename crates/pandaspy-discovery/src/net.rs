//! The real transports: tokio + socket2 + rustls, portable APIs only.
//!
//! Every seam the engine is written against gets exactly one implementation
//! here — [`SystemInterfaces`] for enumeration, [`TokioSsdpStack`] for the
//! multicast sockets, [`TlsCertProbe`] for the 8883 knock — and all three are
//! written with APIs that mean the same thing on macOS, Windows and Linux.
//! Where an operating system genuinely differs, the difference is absorbed by
//! `socket2` or `if-addrs` rather than by a branch in this file. There is no
//! `#[cfg(target_os)]` here and there must never be one; see CLAUDE.md.

use std::fmt;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use rustls::ClientConfig;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::{TcpStream, UdpSocket};
use tokio_rustls::TlsConnector;

use crate::diag::IoFailure;
use crate::discover::{DiscoveryOptions, DiscoveryOutcome, PassiveSetup, SsdpStack, discover};
use crate::interfaces::{InterfaceSource, NetInterface};
use crate::probe::{PortProbe, ProbeOutcome};
use crate::ssdp::{SSDP_MULTICAST_V4, SSDP_PORT, SsdpSocket, multicast_target};

/// How long a TCP connection to port 8883 is given. A printer on the LAN
/// answers in single-digit milliseconds; anything slower is a host that is
/// not there, and the subnet walk has hundreds more addresses to get to.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(800);

/// How long the TLS handshake is given once TCP is up. Longer than the
/// connect budget because a printer mid-print is a busy little machine.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(1500);

// ─── Interfaces ──────────────────────────────────────────────────────────

/// The machine's own interface list, via `if-addrs`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemInterfaces;

impl InterfaceSource for SystemInterfaces {
    fn interfaces(&self) -> io::Result<Vec<NetInterface>> {
        let interfaces = if_addrs::get_if_addrs()?
            .into_iter()
            .filter_map(|interface| {
                let name = interface.name;
                // IPv6 is dropped at the door: Bambu's announcer is IPv4
                // multicast, so an IPv6 address is not a discovery path.
                match interface.addr {
                    if_addrs::IfAddr::V4(v4) => Some(NetInterface {
                        name,
                        ip: v4.ip,
                        netmask: v4.netmask,
                    }),
                    if_addrs::IfAddr::V6(_) => None,
                }
            })
            .collect();
        Ok(interfaces)
    }
}

// ─── SSDP sockets ────────────────────────────────────────────────────────

/// A UDP socket the engine can drive.
#[derive(Debug)]
pub struct TokioSsdpSocket(UdpSocket);

impl SsdpSocket for TokioSsdpSocket {
    async fn send_search(&self, payload: &[u8]) -> io::Result<()> {
        self.0.send_to(payload, multicast_target()).await.map(drop)
    }

    async fn recv(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.0.recv_from(buf).await
    }
}

/// Opens the real sockets discovery runs on.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioSsdpStack;

impl TokioSsdpStack {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl SsdpStack for TokioSsdpStack {
    type Socket = TokioSsdpSocket;

    async fn open_passive(&self, interfaces: &[NetInterface]) -> PassiveSetup<TokioSsdpSocket> {
        let socket = match bind_passive() {
            Ok(socket) => socket,
            // Not an error path in any meaningful sense: Bambu Studio holds
            // port 2021 whenever it is running, and on the platforms where
            // `SO_REUSEADDR` alone does not permit a second binder this is
            // simply what happens. Active search still works, so hand the
            // engine the reason and let it carry on.
            Err(error) => {
                return PassiveSetup {
                    socket: None,
                    bind_failure: Some(IoFailure::from_io(&error)),
                    joins: Vec::new(),
                };
            }
        };

        // Every interface gets its own join, and one refusal must not cost
        // the others: a denied VPN adapter is the normal case on a laptop,
        // and the printer is usually behind one of the interfaces that did
        // succeed. The outcome is keyed by the full `NetInterface`, not its
        // name, because one OS interface can present several IPv4 addresses
        // and the engine must attribute each join to the right one.
        let joins = interfaces
            .iter()
            .map(|interface| {
                let outcome = socket
                    .join_multicast_v4(SSDP_MULTICAST_V4, interface.ip)
                    .map_err(|error| IoFailure::from_io(&error));
                (interface.clone(), outcome)
            })
            .collect();

        PassiveSetup {
            socket: Some(TokioSsdpSocket(socket)),
            bind_failure: None,
            joins,
        }
    }

    async fn open_search(&self, interface: &NetInterface) -> Result<TokioSsdpSocket, IoFailure> {
        bind_search(interface.ip)
            .map(TokioSsdpSocket)
            .map_err(|error| IoFailure::from_io(&error))
    }
}

/// The shared passive listener: bound to the SSDP port on every address, so
/// one socket catches announcements arriving on any interface.
fn bind_passive() -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    // `SO_REUSEADDR` only, never `SO_REUSEPORT`: the latter does not exist on
    // Windows, and a socket option the crate cannot compile everywhere is a
    // platform branch wearing a disguise. The cost is that coexisting with
    // Bambu Studio on the same port is not guaranteed — hence the bind
    // failure being a reported, survivable outcome rather than a panic.
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, SSDP_PORT)).into())?;
    UdpSocket::from_std(std::net::UdpSocket::from(socket))
}

/// A per-interface transmitter: an ephemeral port pinned to one interface, so
/// the M-SEARCH provably leaves through that NIC and its unicast replies come
/// back to a socket that knows which NIC they answer for.
fn bind_search(ip: Ipv4Addr) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::from((ip, 0)).into())?;
    // Without this the kernel picks the outgoing interface from its routing
    // table, which on a machine with a VPN means every search leaves through
    // the tunnel and the printer on the couch never hears one.
    socket.set_multicast_if_v4(&ip)?;
    // Multicast TTL and loopback are left at the OS defaults deliberately.
    // TTL 1 keeps searches on the local segment, which is the only place a
    // LAN-mode printer can be. Loopback-on means the M-SEARCH is delivered
    // back to sockets on this host that have JOINED the group — that is the
    // passive listener (bound to port 2021 and joined on every interface),
    // not this search socket, which only transmits and never joins. So the
    // echo the engine counts as "multicast TX works" arrives on the passive
    // socket; a search socket sees only the unicast 200 OK replies addressed
    // to its ephemeral port.
    UdpSocket::from_std(std::net::UdpSocket::from(socket))
}

// ─── The certificate probe ───────────────────────────────────────────────

/// Accepts every certificate CHAIN presented, but still verifies the
/// handshake signature (see the verify methods below).
///
/// **This is not a TLS client that trusts printers. It is a certificate
/// reader.** The probe's entire job is to obtain the DER a listener on port
/// 8883 serves so its CN can be checked against the shape of a Bambu serial.
/// A printer's certificate is self-signed and generated on the device, so
/// there is no chain to validate and nothing a verifier could usefully say
/// about it — refusing unverifiable certificates here would reject every
/// printer and discover nothing.
///
/// Reading identity is safe precisely because nothing is trusted as a result:
/// the probe sends no credentials, transfers no application data, and closes
/// the connection the moment it has the bytes. Trust is established
/// separately and later, by `pandaspy-client`'s on-first-use pinning, against
/// these same bytes shown to the user.
///
/// **This type must never become visible outside this module, and must never
/// be reachable from MQTT connection code.** The instant a real session
/// borrows it, the access code is handed to whoever answered the port.
#[derive(Debug)]
struct AcceptAnyCertificate {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    // The CHAIN is accepted (a printer's leaf is self-signed), but the
    // handshake SIGNATURE is still verified — delegated to rustls's own
    // checker. The probe only reads a serial and sends nothing, so a stub here
    // would not leak a secret, but it would let a keyless impostor inject a
    // spoofed serial into the discovery list. Verifying the signature means the
    // serial the probe reports belongs to a peer that actually holds the key.
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        // This list goes out in the ClientHello, so it must be the provider's
        // real set or the handshake fails before a certificate ever arrives.
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Knocks on TCP 8883 and reads whatever certificate answers.
pub struct TlsCertProbe {
    connector: TlsConnector,
    connect_timeout: Duration,
    handshake_timeout: Duration,
}

impl TlsCertProbe {
    /// Build the probe. The rustls configuration is assembled once and shared
    /// by every target in a subnet walk — a `ClientConfig` per address would
    /// rebuild the cipher suite tables a thousand times over.
    #[must_use]
    pub fn new() -> Self {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = Arc::new(AcceptAnyCertificate {
            provider: Arc::clone(&provider),
        });
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("the ring provider supports rustls' default protocol versions")
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();

        Self {
            connector: TlsConnector::from(Arc::new(config)),
            connect_timeout: CONNECT_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
        }
    }
}

impl Default for TlsCertProbe {
    fn default() -> Self {
        Self::new()
    }
}

/// `ClientConfig`'s own `Debug` is a page of cipher suites that says nothing
/// about this probe; the timeouts are the only part worth reading in a log.
impl fmt::Debug for TlsCertProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsCertProbe")
            .field("connect_timeout", &self.connect_timeout)
            .field("handshake_timeout", &self.handshake_timeout)
            .finish_non_exhaustive()
    }
}

impl PortProbe for TlsCertProbe {
    async fn probe(&self, target: SocketAddr) -> ProbeOutcome {
        let stream =
            match tokio::time::timeout(self.connect_timeout, TcpStream::connect(target)).await {
                Ok(Ok(stream)) => stream,
                // Refusal is the single most common answer in a subnet walk
                // and it is *good* news: a host exists there. Keeping it
                // distinct from a timeout is what lets diagnostics tell "the
                // subnet is populated but no printer" from "we probed a range
                // where nothing lives".
                Ok(Err(error)) if error.kind() == io::ErrorKind::ConnectionRefused => {
                    return ProbeOutcome::Refused;
                }
                Ok(Err(error)) => return ProbeOutcome::Failed(error.to_string()),
                Err(_elapsed) => return ProbeOutcome::Timeout,
            };

        // No DNS is involved anywhere in discovery: the printer is an address
        // on the LAN, and SNI for an IP literal is not sent.
        let server_name = ServerName::IpAddress(target.ip().into());
        let handshake = tokio::time::timeout(
            self.handshake_timeout,
            self.connector.connect(server_name, stream),
        )
        .await;

        // A handshake that failed and a handshake that never answered are the
        // same finding: something is listening and it does not speak TLS. A
        // plain-TCP service typically just stalls on the ClientHello rather
        // than rejecting it, so treating only the error case as `NotTls`
        // would leave the stall reported as a probe timeout.
        let Ok(Ok(stream)) = handshake else {
            return ProbeOutcome::NotTls;
        };

        match stream
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|chain| chain.first())
        {
            Some(certificate) => ProbeOutcome::TlsPeer {
                cert_der: certificate.to_vec(),
            },
            // Anonymous or PSK-only TLS: a handshake completed but there is
            // no identity to read, so there is nothing here for us.
            None => ProbeOutcome::NotTls,
        }
    }
}

// ─── The convenience entry point ─────────────────────────────────────────

/// Run a discovery pass with the real transports.
///
/// This is the whole crate's public surface as far as `src-tauri` is
/// concerned; everything else exists so that the engine can be tested without
/// it.
pub async fn discover_on_lan(options: &DiscoveryOptions) -> DiscoveryOutcome {
    discover(
        &TokioSsdpStack::new(),
        Arc::new(TlsCertProbe::new()),
        &SystemInterfaces,
        options,
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    use super::*;
    use crate::probe::serial_from_cert;
    use crate::ssdp::SsdpAnnouncement;

    // Every test here binds 127.0.0.1 on port 0 and nothing else. No fixed
    // port, no multicast: a discovery test that needs the machine's real
    // network is a test that fails in CI for reasons that are not bugs.
    //
    // That rule leaves three things in this module permanently uncovered, and
    // they are uncovered by choice rather than by oversight: `bind_passive`
    // (binds the fixed SSDP port), `bind_search` (sets the outgoing multicast
    // interface on a real NIC address) and `send_search` (transmits to the
    // multicast group, which it hardcodes and so cannot be aimed at
    // loopback). Reaching them honestly needs a network namespace, not a
    // cleverer test. They are covered instead by the compiler and by the
    // first real run under `src-tauri`.

    fn self_signed(cn: &str) -> (CertificateDer<'static>, PrivatePkcs8KeyDer<'static>) {
        let mut params = rcgen::CertificateParams::default();
        params.distinguished_name = rcgen::DistinguishedName::new();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, cn);
        let key = rcgen::KeyPair::generate().unwrap();
        let certificate = params.self_signed(&key).unwrap();
        (
            certificate.der().clone(),
            PrivatePkcs8KeyDer::from(key.serialize_der()),
        )
    }

    /// A TLS listener serving a printer-shaped certificate, on a port the OS
    /// picked for us.
    fn tls_server(cn: &str) -> TlsAcceptor {
        let (certificate, key) = self_signed(cn);
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], PrivateKeyDer::Pkcs8(key))
        .unwrap();
        TlsAcceptor::from(Arc::new(config))
    }

    #[tokio::test]
    async fn probing_a_real_tls_endpoint_reads_the_certificate() {
        let acceptor = tls_server("01P00A000000000");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let accepted = acceptor.accept(stream).await;
            // Hold the connection open past the client's side of the
            // handshake; tearing it down the instant the server is finished
            // would race the client's last flight.
            tokio::time::sleep(Duration::from_millis(200)).await;
            drop(accepted);
        });

        let outcome = TlsCertProbe::new().probe(address).await;
        let ProbeOutcome::TlsPeer { cert_der } = outcome else {
            panic!("expected a certificate, got {outcome:?}");
        };
        assert_eq!(
            serial_from_cert(&cert_der).map(|serial| serial.0),
            Some("01P00A000000000".to_owned()),
            "the CN of the served certificate is the printer's serial"
        );
    }

    #[tokio::test]
    async fn probing_a_closed_port_is_refused() {
        // Bind, learn the port, then drop the listener: nothing is listening
        // there, but the address is real and local.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);

        let outcome = TlsCertProbe::new().probe(address).await;
        assert!(
            matches!(outcome, ProbeOutcome::Refused | ProbeOutcome::Timeout),
            "a closed port must not look like a device: {outcome:?}"
        );
    }

    #[tokio::test]
    async fn probing_a_plain_tcp_listener_is_not_tls() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let accepted = listener.accept().await.unwrap();
            // Accept and then say nothing at all — the behaviour of a plain
            // TCP service faced with a ClientHello, and the case that would
            // be misreported as a probe timeout if the handshake timeout were
            // not folded into `NotTls`.
            tokio::time::sleep(Duration::from_secs(30)).await;
            drop(accepted);
        });

        let outcome = TlsCertProbe::new().probe(address).await;
        assert_eq!(outcome, ProbeOutcome::NotTls);
    }

    /// A NOTIFY with no `Location`, so the address the engine ends up with
    /// can only have come from the sender the socket reported.
    ///
    /// Written inline rather than read from `fixtures/ssdp/`: the corpus is
    /// parsed exhaustively by `tests/ssdp_golden.rs`, and its synthetic files
    /// are meant to be *deleted* as real captures replace them. Naming one
    /// here would make that planned deletion break a transport test that has
    /// nothing to do with the corpus.
    const NOTIFY_WITHOUT_LOCATION: &[u8] = b"NOTIFY * HTTP/1.1\r\n\
        HOST: 239.255.255.250:2021\r\n\
        NT: urn:bambulab-com:device:3dprinter:1\r\n\
        USN: 00M09A000000000\r\n\
        DevModel.bambu.com: 3DPrinter-X1-Carbon\r\n\
        \r\n";

    #[tokio::test]
    async fn a_datagram_arriving_on_a_real_socket_becomes_a_printer() {
        // The one part of the SSDP stack that loopback can reach: real bytes
        // over a real UDP socket, through `recv`, out the far end as a
        // printer. Everything either side of this is multicast.
        let listener = TokioSsdpSocket(UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap());
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        sender
            .send_to(NOTIFY_WITHOUT_LOCATION, listener.0.local_addr().unwrap())
            .await
            .unwrap();

        let mut buffer = [0_u8; 2048];
        let (length, from) = listener.recv(&mut buffer).await.unwrap();
        assert_eq!(
            from,
            sender.local_addr().unwrap(),
            "the engine falls back to this address, so it has to be the real one"
        );

        let announcement = SsdpAnnouncement::parse(&buffer[..length]).unwrap();
        let printer = announcement.to_printer(from).unwrap();
        assert_eq!(printer.serial.unwrap().0, "00M09A000000000");
        assert_eq!(printer.address, from.ip());
    }

    #[test]
    fn system_interfaces_enumerates_something() {
        // Deliberately weak: a CI container may legitimately have nothing but
        // loopback, or nothing at all. What is worth asserting is that
        // enumeration succeeds and that every entry is nameable, because a
        // nameless interface breaks the diagnostics keyed on `name`.
        let interfaces = SystemInterfaces.interfaces().unwrap();
        for interface in &interfaces {
            assert!(
                !interface.name.is_empty(),
                "diagnostics key interfaces by name: {interface:?}"
            );
        }
    }
}
