//! The connection lifecycle: an explicit state machine, a generic MQTT session
//! loop, and the supervisor that reconnects.
//!
//! # Layering, for testability
//!
//! * [`run_session`] speaks MQTT over any `AsyncRead + AsyncWrite` stream. It
//!   knows nothing about TCP or TLS, so a test drives it against an in-memory
//!   [`tokio::io::duplex`] with a scripted "printer" on the far end — no
//!   network, no certificates.
//! * [`Transport`] is the seam that produces a connected, TLS-wrapped,
//!   pin-checked stream. The real one lives in [`crate::tls`]; a test supplies
//!   a fake that yields duplexes on a schedule.
//! * [`supervise`] ties them together with [`Backoff`] + [`Jitter`], and is
//!   tested with a fake transport under a paused clock.
//!
//! State transitions ([`ConnectionState`]) are surfaced as [`SessionEvent`]s so
//! the UI can tell "wrong access code" from "printer is off" from "network
//! unreachable" — which is the whole point of making the machine explicit.

use std::fmt;
use std::net::IpAddr;
use std::time::Duration;

use pandaspy_proto::wire::{self, Request};
use pandaspy_proto::{DeviceSerial, PrinterState, StateAccumulator};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::backoff::Backoff;
use crate::jitter::Jitter;
use crate::mqtt::{self, ConnectReturnCode, Packet, Publish, Subscribe};
use crate::pinning::CertificateFingerprint;

/// Where to reach a printer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrinterEndpoint {
    pub address: IpAddr,
    pub port: u16,
    pub serial: DeviceSerial,
}

impl PrinterEndpoint {
    /// The standard endpoint: MQTT-over-TLS on [`wire::MQTT_PORT`].
    #[must_use]
    pub fn new(address: IpAddr, serial: DeviceSerial) -> Self {
        Self {
            address,
            port: wire::MQTT_PORT,
            serial,
        }
    }

    /// Override the port — mainly for tests pointing at a localhost server.
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }
}

/// What the printer wants before it will talk.
///
/// The access code is printed on the printer's screen and is effectively a
/// password. It is held here only for the duration of a connect; the durable
/// copy lives in the OS secret store via `pandaspy-store`.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub access_code: String,
}

impl Credentials {
    /// The LAN-mode credentials: the fixed `bblp` user and the access code.
    #[must_use]
    pub fn lan(access_code: impl Into<String>) -> Self {
        Self {
            username: wire::MQTT_USERNAME.to_owned(),
            access_code: access_code.into(),
        }
    }
}

impl fmt::Debug for Credentials {
    /// Hand-written so an access code cannot reach a log file, a crash report
    /// or a GitHub issue by way of a stray `{:?}`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("access_code", &"<redacted>")
            .finish()
    }
}

/// Tuning for one session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// MQTT keep-alive. A PINGREQ goes out every half of this.
    pub keep_alive: Duration,
    /// How often to re-request a full `pushall`, since deltas are lost during
    /// any silent gap and a long-running print should self-heal.
    pub pushall_interval: Duration,
    /// MQTT client id. Carries a random suffix by default so two PandaSpy
    /// instances watching one printer do not evict each other's session.
    pub client_id: String,
}

impl SessionConfig {
    /// Defaults with a fresh random client id for `serial`.
    #[must_use]
    pub fn for_serial(serial: &DeviceSerial) -> Self {
        Self {
            keep_alive: Duration::from_secs(30),
            pushall_interval: Duration::from_secs(300),
            client_id: random_client_id(serial),
        }
    }
}

fn random_client_id(serial: &DeviceSerial) -> String {
    let mut bytes = [0_u8; 4];
    let _ = getrandom::fill(&mut bytes);
    format!("pandaspy-{}-{:08x}", serial.0, u32::from_le_bytes(bytes))
}

/// The connection lifecycle, surfaced to the UI. Serialisable because it
/// crosses the Tauri event bridge verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Handshaking,
    Connected,
    Failed(FailureReason),
}

/// Why a connection ended — the distinction the user actually needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum FailureReason {
    /// The printer rejected the access code. Fixing it needs the user, so this
    /// is terminal — retrying would just hammer a wrong password.
    WrongAccessCode,
    /// TCP/TLS could not reach the printer: it is off, or the address moved.
    Unreachable { detail: String },
    /// The TLS handshake failed for a reason other than the pin.
    Tls { detail: String },
    /// The pinned certificate changed. Terminal: a reflash and an attacker
    /// look identical, so only the user can approve the new certificate.
    CertificateChanged,
    /// A protocol-level surprise mid-session.
    Protocol { detail: String },
    /// The printer closed the connection, or it dropped.
    ConnectionClosed,
}

impl FailureReason {
    /// Whether the supervisor may reconnect on its own. `false` means the
    /// failure needs a human — a wrong access code or a changed certificate —
    /// and auto-retrying would be either useless or unsafe.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        !matches!(self, Self::WrongAccessCode | Self::CertificateChanged)
    }
}

/// Everything a session tells the rest of the app.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SessionEvent {
    /// A lifecycle transition. `Failed` carries the reason.
    State(ConnectionState),
    /// The accumulated state after a report was folded in. Always a complete
    /// picture — the session owns the [`StateAccumulator`], so subscribers
    /// never see a bare delta.
    Report(Box<PrinterState>),
    /// The session is blocked until the user approves a changed certificate.
    /// Deliberately an event and not a prompt: this crate must not know that
    /// dialogs exist.
    TrustDecisionRequired {
        serial: DeviceSerial,
        pinned: CertificateFingerprint,
        presented: CertificateFingerprint,
    },
}

/// How a transport connect attempt failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransportError {
    /// TCP connect failed or timed out.
    Unreachable(String),
    /// The TLS handshake failed.
    Tls(String),
    /// The certificate did not match the pin. Carries both fingerprints so the
    /// user can compare them against the printer's screen.
    CertificateChanged {
        pinned: CertificateFingerprint,
        presented: CertificateFingerprint,
    },
}

/// Produces a connected, TLS-wrapped, pin-checked byte stream to one printer.
///
/// The seam between the pure session logic and the operating system. The real
/// implementation ([`crate::tls::TlsTransport`]) does TCP + rustls + TOFU
/// pinning; tests supply a fake that hands back an in-memory duplex.
pub trait Transport: Send {
    type Stream: AsyncRead + AsyncWrite + Unpin + Send;

    /// Connect once. Returns a ready stream or why it could not.
    fn connect(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Self::Stream, TransportError>> + Send;
}

/// How a run of [`run_session`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionOutcome {
    /// Shutdown was requested; we left cleanly.
    Shutdown,
    /// The session ended on its own.
    Ended(FailureReason),
}

/// Drive one MQTT session over an already-connected stream until it ends or
/// shutdown is signalled. Emits [`ConnectionState::Handshaking`] then
/// `Connected`, and a [`SessionEvent::Report`] per state report.
///
/// This is where the MQTT protocol lives, and it is deliberately free of TCP
/// and TLS so it can be tested against a duplex.
async fn run_session<S>(
    stream: S,
    endpoint: &PrinterEndpoint,
    credentials: &Credentials,
    config: &SessionConfig,
    events: &mpsc::UnboundedSender<SessionEvent>,
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
) -> SessionOutcome
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut stream = stream;
    let _ = events.send(SessionEvent::State(ConnectionState::Handshaking));

    // ── CONNECT / CONNACK ───────────────────────────────────────────────
    let connect = Packet::Connect(mqtt::Connect {
        client_id: config.client_id.clone(),
        keep_alive_secs: config.keep_alive.as_secs().min(u16::MAX as u64) as u16,
        username: credentials.username.clone(),
        password: credentials.access_code.clone(),
    });
    if mqtt::write_packet(&mut stream, &connect).await.is_err() {
        return SessionOutcome::Ended(FailureReason::ConnectionClosed);
    }

    match mqtt::read_packet(&mut stream).await {
        Ok(Packet::ConnAck(ack)) => match ack.code {
            ConnectReturnCode::Accepted => {}
            ConnectReturnCode::BadCredentials | ConnectReturnCode::NotAuthorized => {
                return SessionOutcome::Ended(FailureReason::WrongAccessCode);
            }
            other => {
                return SessionOutcome::Ended(FailureReason::Protocol {
                    detail: format!("CONNACK refused: {other:?}"),
                });
            }
        },
        Ok(other) => {
            return SessionOutcome::Ended(FailureReason::Protocol {
                detail: format!("expected CONNACK, got {other:?}"),
            });
        }
        Err(_) => return SessionOutcome::Ended(FailureReason::ConnectionClosed),
    }

    // ── SUBSCRIBE / SUBACK ──────────────────────────────────────────────
    let report_topic = wire::report_topic(&endpoint.serial.0);
    let request_topic = wire::request_topic(&endpoint.serial.0);
    let subscribe = Packet::Subscribe(Subscribe {
        packet_id: 1,
        topic: report_topic.clone(),
    });
    if mqtt::write_packet(&mut stream, &subscribe).await.is_err() {
        return SessionOutcome::Ended(FailureReason::ConnectionClosed);
    }
    match mqtt::read_packet(&mut stream).await {
        Ok(Packet::SubAck(ack)) if ack.return_codes.iter().all(|code| *code != 0x80) => {}
        Ok(Packet::SubAck(_)) => {
            return SessionOutcome::Ended(FailureReason::Protocol {
                detail: "printer refused the report subscription".to_owned(),
            });
        }
        Ok(other) => {
            return SessionOutcome::Ended(FailureReason::Protocol {
                detail: format!("expected SUBACK, got {other:?}"),
            });
        }
        Err(_) => return SessionOutcome::Ended(FailureReason::ConnectionClosed),
    }

    // Subscribed: we are live.
    let _ = events.send(SessionEvent::State(ConnectionState::Connected));

    // A fresh snapshot immediately — deltas that arrived before now are gone.
    let mut sequence = 0_u64;
    if send_request(&mut stream, &request_topic, Request::Pushall, &mut sequence)
        .await
        .is_err()
    {
        return SessionOutcome::Ended(FailureReason::ConnectionClosed);
    }

    // ── The live loop ───────────────────────────────────────────────────
    let mut accumulator = StateAccumulator::new();
    let mut ping = tokio::time::interval(config.keep_alive / 2);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping.reset(); // don't fire immediately
    let mut refresh = tokio::time::interval(config.pushall_interval);
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    refresh.reset();

    let mut buffer_stream = stream;
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    let _ = mqtt::write_packet(&mut buffer_stream, &Packet::Disconnect).await;
                    return SessionOutcome::Shutdown;
                }
            }
            _ = ping.tick() => {
                if mqtt::write_packet(&mut buffer_stream, &Packet::PingReq).await.is_err() {
                    return SessionOutcome::Ended(FailureReason::ConnectionClosed);
                }
            }
            _ = refresh.tick() => {
                if send_request(&mut buffer_stream, &request_topic, Request::Pushall, &mut sequence)
                    .await
                    .is_err()
                {
                    return SessionOutcome::Ended(FailureReason::ConnectionClosed);
                }
            }
            packet = mqtt::read_packet(&mut buffer_stream) => {
                match packet {
                    Ok(Packet::Publish(Publish { topic, payload })) if topic == report_topic => {
                        // A malformed payload is a firmware surprise, not a
                        // reason to drop the session — skip it and keep going.
                        if accumulator.apply_payload(&payload).unwrap_or(false)
                            && let Ok(state) = accumulator.state()
                        {
                            let _ = events.send(SessionEvent::Report(Box::new(state)));
                        }
                    }
                    // Anything else on the wire (PINGRESP, an off-topic
                    // PUBLISH) is fine and ignored; the printer talks a lot.
                    Ok(Packet::Disconnect) | Err(_) => {
                        return SessionOutcome::Ended(FailureReason::ConnectionClosed);
                    }
                    Ok(_) => {}
                }
            }
        }
    }
}

async fn send_request<S>(
    stream: &mut S,
    request_topic: &str,
    request: Request,
    sequence: &mut u64,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    *sequence += 1;
    let packet = Packet::Publish(Publish {
        topic: request_topic.to_owned(),
        payload: request.payload(*sequence).into_bytes(),
    });
    mqtt::write_packet(stream, &packet).await
}

/// Everything identifying and configuring one printer's session — the parts
/// that do not change across reconnects. Bundled so [`supervise`] takes a
/// handful of arguments rather than a crowd.
#[derive(Debug, Clone)]
pub struct SessionSpec {
    pub endpoint: PrinterEndpoint,
    pub credentials: Credentials,
    pub config: SessionConfig,
}

/// Supervise one printer: connect, run, and reconnect with backoff + jitter
/// until shutdown or a failure that needs a human.
///
/// One call drives one printer. Watching several means spawning several of
/// these on independent tasks — a stalled or failing printer cannot affect the
/// others because nothing is shared between them.
pub async fn supervise<T, J>(
    mut transport: T,
    spec: SessionSpec,
    events: mpsc::UnboundedSender<SessionEvent>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    mut backoff: Backoff,
    jitter: J,
) where
    T: Transport,
    J: Jitter,
{
    let SessionSpec {
        endpoint,
        credentials,
        config,
    } = spec;
    loop {
        if *shutdown.borrow() {
            break;
        }

        let _ = events.send(SessionEvent::State(ConnectionState::Connecting));

        match transport.connect().await {
            Ok(stream) => {
                backoff.reset();
                match run_session(
                    stream,
                    &endpoint,
                    &credentials,
                    &config,
                    &events,
                    &mut shutdown,
                )
                .await
                {
                    SessionOutcome::Shutdown => {
                        let _ = events.send(SessionEvent::State(ConnectionState::Disconnected));
                        break;
                    }
                    SessionOutcome::Ended(reason) => {
                        let retryable = reason.is_retryable();
                        let _ = events.send(SessionEvent::State(ConnectionState::Failed(reason)));
                        if !retryable {
                            break;
                        }
                    }
                }
            }
            Err(TransportError::CertificateChanged { pinned, presented }) => {
                // Terminal and security-critical: surface both fingerprints and
                // stop. Only the user can approve the new certificate.
                let _ = events.send(SessionEvent::TrustDecisionRequired {
                    serial: endpoint.serial.clone(),
                    pinned,
                    presented,
                });
                let _ = events.send(SessionEvent::State(ConnectionState::Failed(
                    FailureReason::CertificateChanged,
                )));
                break;
            }
            Err(TransportError::Unreachable(detail)) => {
                let _ = events.send(SessionEvent::State(ConnectionState::Failed(
                    FailureReason::Unreachable { detail },
                )));
            }
            Err(TransportError::Tls(detail)) => {
                let _ = events.send(SessionEvent::State(ConnectionState::Failed(
                    FailureReason::Tls { detail },
                )));
            }
        }

        // Wait out the backoff, but wake immediately if asked to shut down.
        let delay = jitter.jitter(backoff.next_delay());
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = shutdown.changed() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_contains_the_access_code() {
        let credentials = Credentials::lan("12345678");
        let rendered = format!("{credentials:?}");
        assert!(!rendered.contains("12345678"), "leaked: {rendered}");
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn terminal_reasons_are_not_retryable() {
        assert!(!FailureReason::WrongAccessCode.is_retryable());
        assert!(!FailureReason::CertificateChanged.is_retryable());
        assert!(FailureReason::ConnectionClosed.is_retryable());
        assert!(
            FailureReason::Unreachable {
                detail: String::new()
            }
            .is_retryable()
        );
    }

    #[test]
    fn connection_state_serialises_with_a_tagged_reason() {
        let json = serde_json::to_string(&ConnectionState::Failed(FailureReason::WrongAccessCode))
            .unwrap();
        assert!(json.contains("\"state\":\"failed\""), "{json}");
        assert!(json.contains("\"reason\":\"wrong-access-code\""), "{json}");
    }

    #[test]
    fn random_client_ids_differ_per_call() {
        let serial = DeviceSerial("00M09A000000000".to_owned());
        let a = random_client_id(&serial);
        let b = random_client_id(&serial);
        assert!(a.starts_with("pandaspy-00M09A000000000-"));
        assert_ne!(a, b, "client ids must not collide across instances");
    }
}
