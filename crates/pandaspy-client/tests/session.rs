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
use tokio::sync::{Mutex, mpsc, watch};

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
const PINGRESP: u8 = 13 << 4;

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

/// Accepts the connection but refuses the subscription (SUBACK 0x80).
async fn subscribe_refusing_printer(mut far: DuplexStream) {
    let _ = read_frame(&mut far).await; // CONNECT
    far.write_all(&frame(CONNACK, &[0x00, 0x00])).await.unwrap();
    let (_, body) = read_frame(&mut far).await; // SUBSCRIBE
    let mut suback = body[0..2].to_vec();
    suback.push(0x80); // subscription failure
    far.write_all(&frame(SUBACK, &suback)).await.unwrap();
    tokio::time::sleep(Duration::from_secs(3600)).await;
}

/// CONNACK with a non-credential refusal (code 3 = server unavailable).
async fn connack_unavailable_printer(mut far: DuplexStream) {
    let _ = read_frame(&mut far).await; // CONNECT
    far.write_all(&frame(CONNACK, &[0x00, 0x03])).await.unwrap();
    tokio::time::sleep(Duration::from_secs(3600)).await;
}

/// Sends a garbage (non-JSON) report, then a good one. The session must skip
/// the garbage and stay live to deliver the good report.
async fn garbage_then_good_printer(mut far: DuplexStream, good_payload: Vec<u8>) {
    let _ = read_frame(&mut far).await; // CONNECT
    far.write_all(&frame(CONNACK, &[0x00, 0x00])).await.unwrap();
    let (_, body) = read_frame(&mut far).await; // SUBSCRIBE
    let mut suback = body[0..2].to_vec();
    suback.push(0x00);
    far.write_all(&frame(SUBACK, &suback)).await.unwrap();
    let _ = read_frame(&mut far).await; // initial pushall

    let topic = pandaspy_proto::wire::report_topic("00M09A000000000");
    for payload in [b"\x00\xffnot json".to_vec(), good_payload] {
        let mut pub_body = mqtt_string(&topic);
        pub_body.extend_from_slice(&payload);
        far.write_all(&frame(PUBLISH, &pub_body)).await.unwrap();
    }
    tokio::time::sleep(Duration::from_secs(3600)).await;
}

/// Completes the MQTT handshake, then drops immediately — a flapping printer.
/// The session reaches Connected but ends at once, so it must NOT reset backoff.
async fn flapping_printer(mut far: DuplexStream) {
    let _ = read_frame(&mut far).await; // CONNECT
    far.write_all(&frame(CONNACK, &[0x00, 0x00])).await.unwrap();
    let (_, body) = read_frame(&mut far).await; // SUBSCRIBE
    let mut suback = body[0..2].to_vec();
    suback.push(0x00);
    far.write_all(&frame(SUBACK, &suback)).await.unwrap();
    // Drop `far` here: the connection closes right after subscribe.
}

/// Reads and counts post-handshake control packets, so a test can assert a
/// keepalive PINGREQ and a periodic pushall actually go out.
async fn counting_printer(
    mut far: DuplexStream,
    pings: Arc<AtomicUsize>,
    pushalls: Arc<AtomicUsize>,
) {
    let _ = read_frame(&mut far).await; // CONNECT
    far.write_all(&frame(CONNACK, &[0x00, 0x00])).await.unwrap();
    let (_, body) = read_frame(&mut far).await; // SUBSCRIBE
    let mut suback = body[0..2].to_vec();
    suback.push(0x00);
    far.write_all(&frame(SUBACK, &suback)).await.unwrap();

    loop {
        let (first, _) = read_frame(&mut far).await;
        match first >> 4 {
            12 => {
                pings.fetch_add(1, Ordering::SeqCst); // PINGREQ
                // Answer it, as a real printer does — otherwise the client's
                // liveness deadline would (correctly) declare the link dead.
                far.write_all(&frame(PINGRESP, &[])).await.unwrap();
            }
            3 => {
                pushalls.fetch_add(1, Ordering::SeqCst); // PUBLISH (pushall request)
            }
            _ => {}
        }
    }
}

// ─── A fake Transport for the supervisor ──────────────────────────────────

struct FakeTransport {
    /// Each call pops the next scripted outcome. `Ok` yields the near half of a
    /// duplex; a task is spawned to drive the far half as a printer.
    script: Vec<TransportStep>,
    calls: Arc<AtomicUsize>,
    connect_times: Arc<Mutex<Vec<tokio::time::Instant>>>,
    report_payload: Vec<u8>,
    pings: Arc<AtomicUsize>,
    pushalls: Arc<AtomicUsize>,
}

#[derive(Clone, Copy)]
enum TransportStep {
    Healthy,
    Reject,
    Unreachable,
    Changed,
    SubscribeRefused,
    ConnackUnavailable,
    GarbageThenGood,
    Flap,
    Counting,
}

impl Transport for FakeTransport {
    type Stream = DuplexStream;

    async fn connect(&mut self) -> Result<DuplexStream, TransportError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.connect_times
            .lock()
            .await
            .push(tokio::time::Instant::now());
        let step = if self.script.is_empty() {
            // After the script, keep failing so the supervisor keeps looping
            // (a test ends it via shutdown).
            TransportStep::Unreachable
        } else {
            self.script.remove(0)
        };
        let spawn_printer = |printer: TransportStep| {
            let (near, far) = tokio::io::duplex(8192);
            match printer {
                TransportStep::Healthy => {
                    tokio::spawn(healthy_printer(far, self.report_payload.clone()));
                }
                TransportStep::Reject => {
                    tokio::spawn(rejecting_printer(far));
                }
                TransportStep::SubscribeRefused => {
                    tokio::spawn(subscribe_refusing_printer(far));
                }
                TransportStep::ConnackUnavailable => {
                    tokio::spawn(connack_unavailable_printer(far));
                }
                TransportStep::GarbageThenGood => {
                    tokio::spawn(garbage_then_good_printer(far, self.report_payload.clone()));
                }
                TransportStep::Flap => {
                    tokio::spawn(flapping_printer(far));
                }
                TransportStep::Counting => {
                    tokio::spawn(counting_printer(
                        far,
                        Arc::clone(&self.pings),
                        Arc::clone(&self.pushalls),
                    ));
                }
                TransportStep::Unreachable | TransportStep::Changed => unreachable!(),
            }
            near
        };
        match step {
            TransportStep::Unreachable => {
                Err(TransportError::Unreachable("no route in test".to_owned()))
            }
            TransportStep::Changed => Err(TransportError::CertificateChanged {
                pinned: pandaspy_client::CertificateFingerprint([1; 32]),
                presented: pandaspy_client::CertificateFingerprint([2; 32]),
            }),
            connecting => Ok(spawn_printer(connecting)),
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
    let connect_times = Arc::new(Mutex::new(Vec::new()));
    let pings = Arc::new(AtomicUsize::new(0));
    let pushalls = Arc::new(AtomicUsize::new(0));
    let transport = FakeTransport {
        script,
        calls: Arc::clone(&calls),
        connect_times: Arc::clone(&connect_times),
        report_payload: report,
        pings: Arc::clone(&pings),
        pushalls: Arc::clone(&pushalls),
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
        connect_times,
        pings,
        pushalls,
    }
}

struct Supervised {
    rx: mpsc::UnboundedReceiver<SessionEvent>,
    shutdown_tx: watch::Sender<bool>,
    handle: tokio::task::JoinHandle<()>,
    calls: Arc<AtomicUsize>,
    connect_times: Arc<Mutex<Vec<tokio::time::Instant>>>,
    pings: Arc<AtomicUsize>,
    pushalls: Arc<AtomicUsize>,
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

#[tokio::test]
async fn a_malformed_report_is_skipped_and_the_session_survives() {
    // A garbage payload mid-stream is a firmware surprise, not a fatal error:
    // the session must skip it and still deliver the good report that follows.
    let mut sup = run_supervisor(vec![TransportStep::GarbageThenGood], sample_report()).await;

    let report = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let SessionEvent::Report(state) = sup.rx.recv().await.unwrap() {
                break state;
            }
        }
    })
    .await
    .expect("the good report after the garbage one should arrive");

    assert_eq!(report.progress_percent(), Some(42));
    let _ = sup.shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), sup.handle).await;
}

#[tokio::test]
async fn a_refused_subscription_is_a_protocol_failure() {
    let mut sup = run_supervisor(vec![TransportStep::SubscribeRefused], sample_report()).await;

    let failed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let SessionEvent::State(ConnectionState::Failed(reason)) =
                sup.rx.recv().await.unwrap()
            {
                break reason;
            }
        }
    })
    .await
    .expect("a refused subscription should surface a failure");

    assert!(
        matches!(failed, FailureReason::Protocol { .. }),
        "expected Protocol, got {failed:?}"
    );
    let _ = sup.shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), sup.handle).await;
}

#[tokio::test]
async fn a_non_credential_connack_refusal_is_a_protocol_failure() {
    // CONNACK code 3 (server unavailable) is not a wrong access code — it is a
    // retryable protocol-level refusal, distinct from WrongAccessCode.
    let mut sup = run_supervisor(vec![TransportStep::ConnackUnavailable], sample_report()).await;

    let failed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let SessionEvent::State(ConnectionState::Failed(reason)) =
                sup.rx.recv().await.unwrap()
            {
                break reason;
            }
        }
    })
    .await
    .expect("an unavailable CONNACK should surface a failure");

    assert!(
        matches!(failed, FailureReason::Protocol { .. }),
        "expected Protocol, got {failed:?}"
    );
    assert!(
        failed.is_retryable(),
        "server-unavailable should be retried"
    );
    let _ = sup.shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), sup.handle).await;
}

#[tokio::test(start_paused = true)]
async fn a_flapping_printer_backs_off_instead_of_hammering() {
    // A printer that completes the handshake then drops immediately must NOT
    // reset the backoff curve — otherwise it is reconnected at the base rate
    // forever. The gaps between attempts must grow.
    let sup = run_supervisor(vec![TransportStep::Flap; 6], sample_report()).await;

    // Let several flap/backoff cycles play out on the paused clock.
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            if sup.calls.load(Ordering::SeqCst) >= 4 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the supervisor should keep retrying a flapping printer");

    let _ = sup.shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), sup.handle).await;

    let times = sup.connect_times.lock().await;
    assert!(times.len() >= 4, "need several attempts to measure backoff");
    let gap1 = times[1] - times[0];
    let gap3 = times[3] - times[2];
    assert!(
        gap3 > gap1,
        "backoff must grow across flaps (gap1={gap1:?}, gap3={gap3:?}); \
         equal gaps mean the curve was wrongly reset on each bare connect"
    );
}

#[tokio::test(start_paused = true)]
async fn keepalive_pings_and_periodic_pushalls_are_sent() {
    // The printer counts what the client sends after subscribing: a keepalive
    // PINGREQ (every keep_alive/2) and more than the one initial pushall (the
    // periodic refresh). Under the paused clock the intervals elapse instantly.
    let sup = run_supervisor(vec![TransportStep::Counting], sample_report()).await;

    tokio::time::timeout(Duration::from_secs(700), async {
        loop {
            if sup.pings.load(Ordering::SeqCst) >= 1 && sup.pushalls.load(Ordering::SeqCst) >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
    .await
    .expect("a keepalive ping and a periodic pushall should both go out");

    let _ = sup.shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), sup.handle).await;
}
