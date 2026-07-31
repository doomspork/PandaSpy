//! The real [`Transport`]: TCP + rustls (ring) + trust-on-first-use pinning.
//!
//! A printer's certificate is self-signed and generated on the device, so there
//! is no chain to validate — ordinary verification would reject every printer.
//! We accept any certificate at the rustls layer, then, *before sending the
//! access code*, hash the leaf and check it against the pin store. Order
//! matters: a changed certificate aborts the connection with the credentials
//! still unsent.

use std::sync::Arc;
use std::time::Duration;

use rustls::ClientConfig;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::pinning::{self, CertificateFingerprint, PinStore, TrustDecision};
use crate::session::{PrinterEndpoint, Transport, TransportError};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Connects to one printer over TLS and enforces its pin.
#[derive(Clone)]
pub struct TlsTransport {
    endpoint: PrinterEndpoint,
    pins: Arc<dyn PinStore>,
    connector: TlsConnector,
    connect_timeout: Duration,
    handshake_timeout: Duration,
}

impl std::fmt::Debug for TlsTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsTransport")
            .field("endpoint", &self.endpoint)
            .field("pins", &self.pins)
            .finish_non_exhaustive()
    }
}

impl TlsTransport {
    /// Build a transport for one printer, sharing a pin store.
    #[must_use]
    pub fn new(endpoint: PrinterEndpoint, pins: Arc<dyn PinStore>) -> Self {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = Arc::new(AcceptAnyServerCert {
            provider: Arc::clone(&provider),
        });
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("the ring provider supports rustls' default protocol versions")
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();

        Self {
            endpoint,
            pins,
            connector: TlsConnector::from(Arc::new(config)),
            connect_timeout: CONNECT_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
        }
    }
}

impl Transport for TlsTransport {
    type Stream = TlsStream<TcpStream>;

    async fn connect(&mut self) -> Result<Self::Stream, TransportError> {
        let addr = (self.endpoint.address, self.endpoint.port);

        let tcp = match tokio::time::timeout(self.connect_timeout, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => return Err(TransportError::Unreachable(error.to_string())),
            Err(_) => return Err(TransportError::Unreachable("connect timed out".to_owned())),
        };

        let server_name = ServerName::IpAddress(self.endpoint.address.into());
        let tls = match tokio::time::timeout(
            self.handshake_timeout,
            self.connector.connect(server_name, tcp),
        )
        .await
        {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => return Err(TransportError::Tls(error.to_string())),
            Err(_) => return Err(TransportError::Tls("handshake timed out".to_owned())),
        };

        // The leaf certificate is the printer's identity. Hash it and decide
        // trust before a single credential byte is written.
        let leaf = tls
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|chain| chain.first())
            .ok_or_else(|| TransportError::Tls("printer presented no certificate".to_owned()))?;
        let presented = CertificateFingerprint::of_der(leaf);

        match pinning::evaluate(self.pins.as_ref(), &self.endpoint.serial, presented) {
            TrustDecision::Trusted => Ok(tls),
            TrustDecision::FirstSight(fingerprint) => {
                // Record the pin now, so the next connection can require it.
                self.pins
                    .pin(&self.endpoint.serial, fingerprint)
                    .map_err(|error| {
                        TransportError::Tls(format!("could not persist pin: {error}"))
                    })?;
                Ok(tls)
            }
            TrustDecision::Changed { pinned, presented } => {
                Err(TransportError::CertificateChanged { pinned, presented })
            }
        }
    }
}

/// Accepts any server certificate. **Reads identity; never establishes trust.**
///
/// Trust is the pin check in [`TlsTransport::connect`], which runs before the
/// access code is sent. This verifier must never be used by code that would
/// transmit credentials without that subsequent pin check.
#[derive(Debug)]
struct AcceptAnyServerCert {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for AcceptAnyServerCert {
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

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::Ipv4Addr;
    use std::sync::Mutex;

    use pandaspy_proto::DeviceSerial;
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    use super::*;
    use crate::pinning::PinStoreError;

    #[derive(Debug, Default)]
    struct MemoryPins(Mutex<HashMap<String, CertificateFingerprint>>);

    impl PinStore for MemoryPins {
        fn pinned(&self, serial: &DeviceSerial) -> Option<CertificateFingerprint> {
            self.0.lock().unwrap().get(&serial.0).copied()
        }
        fn pin(
            &self,
            serial: &DeviceSerial,
            fingerprint: CertificateFingerprint,
        ) -> Result<(), PinStoreError> {
            self.0.lock().unwrap().insert(serial.0.clone(), fingerprint);
            Ok(())
        }
    }

    /// A localhost TLS server serving a fresh self-signed cert; returns the
    /// listener, the acceptor, and the cert's DER (so a test can pre-pin it).
    fn tls_server() -> (TlsAcceptor, Vec<u8>) {
        let mut params = rcgen::CertificateParams::default();
        params.distinguished_name = rcgen::DistinguishedName::new();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "00M09A000000000");
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        let cert_der = cert.der().to_vec();

        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert_der.clone())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
        .unwrap();
        (TlsAcceptor::from(Arc::new(config)), cert_der)
    }

    async fn serve_once(listener: TcpListener, acceptor: TlsAcceptor) {
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut tls) = acceptor.accept(stream).await
            {
                // Hold the connection briefly so the client finishes its side
                // of the handshake and its pin check.
                let mut buf = [0_u8; 16];
                let _ = tokio::time::timeout(Duration::from_millis(200), tls.read(&mut buf)).await;
            }
        });
    }

    fn transport_to(port: u16, pins: Arc<dyn PinStore>) -> TlsTransport {
        TlsTransport::new(
            PrinterEndpoint::new(
                Ipv4Addr::LOCALHOST.into(),
                DeviceSerial("00M09A000000000".to_owned()),
            )
            .with_port(port),
            pins,
        )
    }

    #[tokio::test]
    async fn first_sight_pins_the_certificate_and_connects() {
        let (acceptor, cert_der) = tls_server();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        serve_once(listener, acceptor).await;

        let pins: Arc<dyn PinStore> = Arc::new(MemoryPins::default());
        let mut transport = transport_to(port, Arc::clone(&pins));
        assert!(transport.connect().await.is_ok());

        // The leaf we served is now pinned.
        let serial = DeviceSerial("00M09A000000000".to_owned());
        assert_eq!(
            pins.pinned(&serial),
            Some(CertificateFingerprint::of_der(&cert_der))
        );
    }

    #[tokio::test]
    async fn a_matching_pin_connects_again() {
        let (acceptor, cert_der) = tls_server();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        serve_once(listener, acceptor).await;

        let pins = MemoryPins::default();
        pins.pin(
            &DeviceSerial("00M09A000000000".to_owned()),
            CertificateFingerprint::of_der(&cert_der),
        )
        .unwrap();
        let mut transport = transport_to(port, Arc::new(pins));
        assert!(transport.connect().await.is_ok());
    }

    #[tokio::test]
    async fn a_changed_certificate_is_refused_with_both_fingerprints() {
        let (acceptor, _cert_der) = tls_server();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        serve_once(listener, acceptor).await;

        // Pin a DIFFERENT fingerprint than the server will present.
        let stale = CertificateFingerprint([0x00; 32]);
        let pins = MemoryPins::default();
        pins.pin(&DeviceSerial("00M09A000000000".to_owned()), stale)
            .unwrap();

        let mut transport = transport_to(port, Arc::new(pins));
        match transport.connect().await {
            Err(TransportError::CertificateChanged { pinned, presented }) => {
                assert_eq!(pinned, stale);
                assert_ne!(presented, stale);
            }
            other => panic!("expected CertificateChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_unreachable_address_is_reported_as_unreachable() {
        // Bind then drop, so the port is closed but local.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let mut transport = transport_to(port, Arc::new(MemoryPins::default()));
        assert!(matches!(
            transport.connect().await,
            Err(TransportError::Unreachable(_))
        ));
    }
}
