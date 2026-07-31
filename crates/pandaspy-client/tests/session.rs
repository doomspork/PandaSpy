//! The session and supervisor, driven end to end without a network.
//!
//! `run_session` is exercised over an in-memory `tokio::io::duplex` with a
//! scripted MQTT "printer" on the far end; the supervisor is exercised against
//! a fake `Transport` under a paused clock. Between them they cover the MQTT
//! handshake, the report path, the credential/refusal paths, and the
//! reconnect/backoff/terminal-failure logic — all deterministically.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pandaspy_client::{
    Backoff, ConnectionState, Credentials, FailureReason, NoJitter, PrinterEndpoint, SessionConfig,
    SessionEvent, SessionSpec, Transport, TransportError, supervise,
};
use pandaspy_proto::DeviceSerial;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::{mpsc, watch};

// The client's mqtt module is private, so the fake printer re-encodes packets
// by hand at the wire level — which doubles as an independent check that our
// encoder and the wire format agree.

fn serial() -> DeviceSerial {
    DeviceSerial("00M09A000000000".to_owned())
}

fn endpoint() -> PrinterEndpoint {
    PrinterEndpoint::new(std::net::Ipv4Addr::LOCALHOST.into(), serial())
}

fn config() -> SessionConfig {
    SessionConfig {
        keep_alive: Duration::from_secs(30),
        pushall_interval: Duration::from_secs(300),
        client_id: "pandaspy-test".to_owned(),
    }
}

// ─── A minimal wire-level MQTT helper for the fake printer ────────────────

fn remaining_length(mut len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (len % 128) as u8;
        len /= 128;
        if len > 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if len == 0 {
            break;
        }
    }
    out
}

fn frame(type_and_flags: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![type_and_flags];
    out.extend(remaining_length(body.len()));
    out.extend_from_slice(body);
    out
}

fn mqtt_string(s: &str) -> Vec<u8> {
    let mut out = (s.len() as u16).to_be_bytes().to_vec();
    out.extend_from_slice(s.as_bytes());
    out
}

/// Read one packet's (first_byte, body) from the far end.
async fn read_frame(stream: &mut DuplexStream) -> (u8, Vec<u8>) {
    let first = stream.read_u8().await.unwrap();
    let mut len = 0_usize;
    let mut mult = 1_usize;
    loop {
        let b = stream.read_u8().await.unwrap();
        len += (b & 0x7f) as usize * mult;
        if b & 0x80 == 0 {
            break;
        }
        mult *= 128;
    }
    let mut body = vec![0_u8; len];
    stream.read_exact(&mut body).await.unwrap();
    (first, body)
}

const CONNACK: u8 = 2 << 4;
const SUBACK: u8 = 9 << 4;
const PUBLISH: u8 = 3 << 4;

/// A fake printer that accepts, subscribes, then pushes one report.
async fn healthy_printer(mut far: DuplexStream, report_payload: Vec<u8>) {
    // Expect CONNECT.
    let (first, _) = read_frame(&mut far).await;
    assert_eq!(first >> 4, 1, "expected CONNECT");
    far.write_all(&frame(CONNACK, &[0x00, 0x00])).await.unwrap(); // accepted

    // Expect SUBSCRIBE; reply SUBACK granting QoS 0.
    let (first, body) = read_frame(&mut far).await;
    assert_eq!(first >> 4, 8, "expected SUBSCRIBE");
    let packet_id = &body[0..2];
    let mut suback = packet_id.to_vec();
    suback.push(0x00);
    far.write_all(&frame(SUBACK, &suback)).await.unwrap();

    // The client sends an initial pushall request (a PUBLISH); consume it.
    let (first, _) = read_frame(&mut far).await;
    assert_eq!(first >> 4, 3, "expected the pushall PUBLISH");

    // Push one report on the report topic.
    let topic = pandaspy_proto::wire::report_topic("00M09A000000000");
    let mut pub_body = mqtt_string(&topic);
    pub_body.extend_from_slice(&report_payload);
    far.write_all(&frame(PUBLISH, &pub_body)).await.unwrap();

    // Keep the connection open so the client stays live until shutdown.
    tokio::time::sleep(Duration::from_secs(3600)).await;
}

/// A fake printer that rejects the credentials.
async fn rejecting_printer(mut far: DuplexStream) {
    let (first, _) = read_frame(&mut far).await;
    assert_eq!(first >> 4, 1, "expected CONNECT");
    // CONNACK return code 4 = bad username/password.
    far.write_all(&frame(CONNACK, &[0x00, 0x04])).await.unwrap();
}

// ─── A fake Transport for the supervisor ──────────────────────────────────

struct FakeTransport {
    /// Each call pops the next scripted outcome. `Ok` yields the near half of a
    /// duplex; a task is spawned to drive the far half as a printer.
    script: Vec<TransportStep>,
    calls: Arc<AtomicUsize>,
    report_payload: Vec<u8>,
}

enum TransportStep {
    Healthy,
    Reject,
    Unreachable,
    Changed,
}

impl Transport for FakeTransport {
    type Stream = DuplexStream;

    async fn connect(&mut self) -> Result<DuplexStream, TransportError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let step = if self.script.is_empty() {
            // After the script, keep failing so the supervisor keeps looping
            // (a test ends it via shutdown).
            TransportStep::Unreachable
        } else {
            self.script.remove(0)
        };
        match step {
            TransportStep::Healthy => {
                let (near, far) = tokio::io::duplex(8192);
                let payload = self.report_payload.clone();
                tokio::spawn(healthy_printer(far, payload));
                Ok(near)
            }
            TransportStep::Reject => {
                let (near, far) = tokio::io::duplex(8192);
                tokio::spawn(rejecting_printer(far));
                Ok(near)
            }
            TransportStep::Unreachable => {
                Err(TransportError::Unreachable("no route in test".to_owned()))
            }
            TransportStep::Changed => Err(TransportError::CertificateChanged {
                pinned: pandaspy_client::CertificateFingerprint([1; 32]),
                presented: pandaspy_client::CertificateFingerprint([2; 32]),
            }),
        }
    }
}

fn drain(rx: &mut mpsc::UnboundedReceiver<SessionEvent>) -> Vec<SessionEvent> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        out.push(event);
    }
    out
}

fn states(events: &[SessionEvent]) -> Vec<ConnectionState> {
    events
        .iter()
        .filter_map(|e| match e {
            SessionEvent::State(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

async fn run_supervisor(script: Vec<TransportStep>, report: Vec<u8>) -> Supervised {
    let calls = Arc::new(AtomicUsize::new(0));
    let transport = FakeTransport {
        script,
        calls: Arc::clone(&calls),
        report_payload: report,
    };
    let (tx, rx) = mpsc::unbounded_channel();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let handle = tokio::spawn(supervise(
        transport,
        SessionSpec {
            endpoint: endpoint(),
            credentials: Credentials::lan("12345678"),
            config: config(),
        },
        tx,
        shutdown_rx,
        Backoff::new(Duration::from_secs(1), Duration::from_secs(30)),
        NoJitter,
    ));
    Supervised {
        rx,
        shutdown_tx,
        handle,
        calls,
    }
}

struct Supervised {
    rx: mpsc::UnboundedReceiver<SessionEvent>,
    shutdown_tx: watch::Sender<bool>,
    handle: tokio::task::JoinHandle<()>,
    calls: Arc<AtomicUsize>,
}

// A report payload the accumulator will accept and expose.
fn sample_report() -> Vec<u8> {
    br#"{"print":{"command":"push_status","gcode_state":"RUNNING","mc_percent":42}}"#.to_vec()
}

#[tokio::test]
async fn a_healthy_session_connects_subscribes_and_reports() {
    let mut sup = run_supervisor(vec![TransportStep::Healthy], sample_report()).await;

    // Wait for a Report to arrive (bounded, real time — this test is not paused).
    let report = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match sup.rx.recv().await.unwrap() {
                SessionEvent::Report(state) => break state,
                _ => continue,
            }
        }
    })
    .await
    .expect("a report should arrive");

    assert_eq!(report.progress_percent(), Some(42));

    // A Report only arrives after CONNECT/SUBSCRIBE succeeded, so the lifecycle
    // necessarily passed through Connected by now.
    let _ = sup.shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), sup.handle).await;
}

#[tokio::test]
async fn wrong_credentials_fail_terminally_without_retrying() {
    let mut sup = run_supervisor(vec![TransportStep::Reject], sample_report()).await;

    // The supervisor should stop on its own (terminal failure) — join returns.
    tokio::time::timeout(Duration::from_secs(5), sup.handle)
        .await
        .expect("supervisor must exit on a terminal failure")
        .unwrap();

    let events = drain(&mut sup.rx);
    let failed = states(&events)
        .into_iter()
        .any(|s| s == ConnectionState::Failed(FailureReason::WrongAccessCode));
    assert!(failed, "expected a WrongAccessCode failure: {events:?}");
    assert_eq!(
        sup.calls.load(Ordering::SeqCst),
        1,
        "a wrong access code must NOT be retried"
    );
}

#[tokio::test]
async fn a_changed_certificate_stops_and_asks_the_user() {
    let mut sup = run_supervisor(vec![TransportStep::Changed], sample_report()).await;

    tokio::time::timeout(Duration::from_secs(5), sup.handle)
        .await
        .expect("supervisor must exit on a changed certificate")
        .unwrap();

    let events = drain(&mut sup.rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::TrustDecisionRequired { .. })),
        "the user must be asked: {events:?}"
    );
    assert_eq!(sup.calls.load(Ordering::SeqCst), 1, "never auto-retried");
}

#[tokio::test(start_paused = true)]
async fn an_unreachable_printer_is_retried_with_backoff() {
    // Fail (unreachable) twice, then succeed. Under a paused clock the backoff
    // sleeps are advanced automatically by tokio's auto-advance.
    let mut sup = run_supervisor(
        vec![
            TransportStep::Unreachable,
            TransportStep::Unreachable,
            TransportStep::Healthy,
        ],
        sample_report(),
    )
    .await;

    let report = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            match sup.rx.recv().await.unwrap() {
                SessionEvent::Report(state) => break state,
                _ => continue,
            }
        }
    })
    .await
    .expect("after retries, a report should arrive");

    assert_eq!(report.progress_percent(), Some(42));
    assert!(
        sup.calls.load(Ordering::SeqCst) >= 3,
        "should have retried past the two unreachable attempts"
    );

    let _ = sup.shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), sup.handle).await;
}

#[tokio::test]
async fn shutdown_ends_a_live_session_cleanly() {
    let mut sup = run_supervisor(vec![TransportStep::Healthy], sample_report()).await;

    // Wait until connected (a report proves the session is live).
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let SessionEvent::Report(_) = sup.rx.recv().await.unwrap() {
                break;
            }
        }
    })
    .await
    .unwrap();

    sup.shutdown_tx.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), sup.handle)
        .await
        .expect("shutdown must end the supervisor")
        .unwrap();

    let events = drain(&mut sup.rx);
    assert!(
        states(&events)
            .into_iter()
            .any(|s| s == ConnectionState::Disconnected),
        "a clean shutdown reports Disconnected: {events:?}"
    );
}
