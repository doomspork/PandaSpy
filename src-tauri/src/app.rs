//! The integration layer: Rust owns all printer state, and the frontend is a
//! view of it.
//!
//! This is the seam the domain crates were shaped for. It selects the secret
//! backend at runtime, adapts `pandaspy-store`'s pin persistence to the client's
//! `PinStore` trait, spawns one `pandaspy-client` supervisor per configured
//! printer, and forwards every `SessionEvent` to the window as a Tauri event.
//! The window never opens a socket; it renders what it is told.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pandaspy_client::{
    Backoff, CertificateFingerprint, Credentials, EqualJitter, PinStore, PinStoreError,
    PrinterEndpoint, SessionConfig, SessionEvent, SessionSpec, TlsTransport, supervise,
};
use pandaspy_proto::{DeviceSerial, PrinterState};
use pandaspy_store::{
    CertPinStore, Config, ConfigStore, EncryptedFileSecrets, FileConfigStore, KeyringSecrets,
    PrinterEntry, SecretBackend, SecretStore, keyring_available,
};
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;

use crate::view::{ConnectionView, PrinterSnapshot, PrinterView};

/// Tauri event names. Kept as constants so the emitter and `ipc.ts` cannot
/// drift apart silently.
pub mod events {
    /// One printer's connection and/or state changed. Payload: [`PrinterView`].
    pub const PRINTER_UPDATE: &str = "printer://update";
    /// A printer's certificate changed and needs the user's decision.
    pub const TRUST_REQUIRED: &str = "printer://trust-required";
}

/// The whole runtime, behind Tauri's managed state.
pub struct AppState {
    config: Mutex<Config>,
    config_store: FileConfigStore,
    secrets: Arc<dyn SecretStore>,
    backend: SecretBackend,
    pins: Arc<PinStoreAdapter>,
    /// serial → live handle. `Mutex` (not async) because every touch is a quick
    /// map edit, never held across `.await`.
    printers: Mutex<HashMap<String, PrinterHandle>>,
    /// Monotonic source of per-handle generations. A replacement (re-add,
    /// resolve-trust) installs a new handle under the same serial; the
    /// generation lets a superseded session's forwarder recognise it no longer
    /// owns the entry and stop writing to it. See [`AppState::spawn_supervisor`].
    next_generation: AtomicU64,
    /// Active UI locale, used to resolve HMS/error text into views.
    lang: Mutex<String>,
    app: AppHandle,
}

/// One running printer: its latest observed connection/state and the switch that
/// stops its supervisor. Identity and order live in [`Config`], the source of
/// truth [`AppState::printer_views`] reads; the handle holds only what the
/// session produces at runtime.
struct PrinterHandle {
    connection: ConnectionView,
    state: Option<PrinterState>,
    shutdown: watch::Sender<bool>,
    /// Which supervisor generation owns this entry. A forwarder only mutates the
    /// handle when its captured generation still matches, so a stopped-then-
    /// replaced session cannot write stale state into its successor.
    generation: u64,
}

impl AppState {
    /// Build the runtime: load config, choose the secret backend, open the pin
    /// store. Does **not** start any sessions — call [`start`](Self::start)
    /// after the state is managed, so the per-session event forwarders can look
    /// it up via `try_state`.
    pub fn load(app: AppHandle, lang: String) -> Self {
        let base = config_dir(&app);
        let config_store = FileConfigStore::new(base.join("config.json"));
        let config = config_store.load().unwrap_or_else(|error| {
            // A corrupt config must not wipe the user's printer list on sight —
            // but at startup there is no UI yet to ask. Log and start empty
            // rather than crash; the file is left on disk for inspection.
            eprintln!("[config] {error}; starting with defaults");
            Config::default()
        });

        let (secrets, backend) = choose_secret_backend(&base);
        let pins = Arc::new(PinStoreAdapter::new(CertPinStore::new(
            base.join("pins.json"),
        )));

        Self {
            config: Mutex::new(config),
            config_store,
            secrets,
            backend,
            pins,
            printers: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(0),
            lang: Mutex::new(lang),
            app,
        }
    }

    /// A fresh, unique generation for a newly-installed printer handle.
    fn next_generation(&self) -> u64 {
        self.next_generation.fetch_add(1, Ordering::Relaxed)
    }

    /// Bring every configured printer online.
    ///
    /// Deliberately separate from [`load`](Self::load) and called *after* the
    /// state is managed: each session spawns a forwarder that resolves the state
    /// with `try_state`, which is empty until `manage` has run. Starting sessions
    /// inside `load` would race the first `Connecting` event against `manage`.
    pub fn start(&self) {
        let entries: Vec<PrinterEntry> = self.config.lock().unwrap().printers.clone();
        for entry in entries {
            if entry.serial.is_none() {
                continue;
            }
            let serial = entry.serial.as_ref().unwrap().0.clone();
            // A printer with no stored access code cannot connect; keep it
            // listed (Disconnected) so the user can supply one.
            match self.secrets.access_code(&serial) {
                Ok(code) => self.spawn_supervisor(entry, code),
                Err(_) => self.insert_idle(entry),
            }
        }
    }

    /// Which backend secrets landed in, for the settings screen.
    #[must_use]
    pub fn secret_backend(&self) -> SecretBackend {
        self.backend
    }

    /// The stored locale override, if any (`None` means "follow the OS").
    #[must_use]
    pub fn stored_locale(&self) -> Option<String> {
        self.config.lock().unwrap().locale.clone()
    }

    /// The serials of every configured printer, for dedup in discovery.
    #[must_use]
    pub fn known_serials(&self) -> Vec<String> {
        self.config
            .lock()
            .unwrap()
            .printers
            .iter()
            .filter_map(|p| p.serial.as_ref().map(|s| s.0.clone()))
            .collect()
    }

    /// The persisted settings: the locale override and launch-at-login flag.
    #[must_use]
    pub fn settings(&self) -> (Option<String>, bool) {
        let config = self.config.lock().unwrap();
        (
            config.locale.clone(),
            config.launch_at_login.unwrap_or(false),
        )
    }

    /// Persist the locale override and launch-at-login preference.
    ///
    /// # Errors
    ///
    /// If the config cannot be saved.
    pub fn set_settings(
        &self,
        locale: Option<String>,
        launch_at_login: bool,
    ) -> Result<(), String> {
        let mut config = self.config.lock().unwrap();
        config.locale = locale;
        config.launch_at_login = Some(launch_at_login);
        self.config_store.save(&config).map_err(|e| e.to_string())
    }

    /// Set the locale used to resolve HMS/error text in future views.
    pub fn set_lang(&self, lang: String) {
        *self.lang.lock().unwrap() = lang;
    }

    /// Snapshot every configured printer for the UI, in configured order.
    #[must_use]
    pub fn printer_views(&self) -> Vec<PrinterView> {
        // Collect everything the mapping needs from under the locks, then drop
        // them before calling into `pandaspy-proto`. Mapping outside the locks
        // keeps the hold short and, defensively, means a panic inside the proto
        // mapping on a hostile payload cannot poison these mutexes and wedge
        // every other command and forwarder.
        let lang = self.lang.lock().unwrap().clone();
        let raw: Vec<(PrinterEntry, ConnectionView, Option<PrinterState>)> = {
            let printers = self.printers.lock().unwrap();
            let config = self.config.lock().unwrap();
            config
                .printers
                .iter()
                .map(|entry| {
                    let handle = entry.serial.as_ref().and_then(|s| printers.get(&s.0));
                    let connection = handle
                        .map(|h| h.connection.clone())
                        .unwrap_or(ConnectionView::DISCONNECTED);
                    let state = handle.and_then(|h| h.state.clone());
                    (entry.clone(), connection, state)
                })
                .collect()
        };

        raw.into_iter()
            .filter_map(|(entry, connection, state)| {
                let serial = entry.serial.as_ref()?.0.clone();
                Some(PrinterView {
                    serial,
                    nickname: entry.nickname.clone(),
                    model: None,
                    address: entry.last_address.clone(),
                    connection,
                    state: state
                        .as_ref()
                        .map(|s| PrinterSnapshot::from_state(s, &lang)),
                })
            })
            .collect()
    }

    /// Add (or re-add) a printer: persist the secret and config entry, then
    /// bring its session up.
    ///
    /// # Errors
    ///
    /// If the secret cannot be stored or the config cannot be saved.
    pub fn add_printer(
        &self,
        serial: String,
        address: String,
        access_code: String,
        nickname: Option<String>,
    ) -> Result<(), String> {
        self.secrets
            .set_access_code(&serial, &access_code)
            .map_err(|e| e.to_string())?;

        let entry = PrinterEntry {
            serial: Some(DeviceSerial(serial.clone())),
            last_address: Some(address),
            nickname,
        };
        {
            let mut config = self.config.lock().unwrap();
            config
                .printers
                .retain(|p| p.serial.as_ref().map(|s| &s.0) != Some(&serial));
            config.printers.push(entry.clone());
            self.config_store.save(&config).map_err(|e| e.to_string())?;
        }

        self.stop_supervisor(&serial);
        self.spawn_supervisor(entry, access_code);
        Ok(())
    }

    /// Remove a printer entirely: stop it, and forget its secret and pin.
    ///
    /// # Errors
    ///
    /// If the config cannot be saved. Secret/pin removal failures are best
    /// effort — a leftover keychain entry is not worth failing the removal.
    pub fn remove_printer(&self, serial: &str) -> Result<(), String> {
        self.stop_supervisor(serial);
        {
            let mut config = self.config.lock().unwrap();
            config
                .printers
                .retain(|p| p.serial.as_ref().map(|s| s.0.as_str()) != Some(serial));
            self.config_store.save(&config).map_err(|e| e.to_string())?;
        }
        let _ = self.secrets.forget(serial);
        let _ = self.pins.inner.forget(serial);
        self.printers.lock().unwrap().remove(serial);
        Ok(())
    }

    /// Reorder the printer list to match `serials`; unknown serials are ignored
    /// and any omitted are appended in their previous order.
    ///
    /// # Errors
    ///
    /// If the reordered config cannot be saved.
    pub fn reorder(&self, serials: &[String]) -> Result<(), String> {
        let mut config = self.config.lock().unwrap();
        let rank = |entry: &PrinterEntry| {
            entry
                .serial
                .as_ref()
                .and_then(|s| serials.iter().position(|o| o == &s.0))
                .unwrap_or(usize::MAX)
        };
        config.printers.sort_by_key(rank);
        self.config_store.save(&config).map_err(|e| e.to_string())
    }

    /// Respond to a changed-certificate prompt. Accepting forgets the old pin so
    /// the next connect trusts the new certificate on first sight and re-pins;
    /// rejecting leaves the printer stopped.
    ///
    /// # Errors
    ///
    /// If the access code cannot be read back to restart the session.
    pub fn resolve_trust(&self, serial: &str, accept: bool) -> Result<(), String> {
        if !accept {
            return Ok(());
        }
        let _ = self.pins.inner.forget(serial);
        let entry = {
            let config = self.config.lock().unwrap();
            config
                .printers
                .iter()
                .find(|p| p.serial.as_ref().map(|s| s.0.as_str()) == Some(serial))
                .cloned()
        };
        if let Some(entry) = entry {
            let code = self
                .secrets
                .access_code(serial)
                .map_err(|e| e.to_string())?;
            self.stop_supervisor(serial);
            self.spawn_supervisor(entry, code);
        }
        Ok(())
    }

    /// Record an idle (unconnected) printer so it still appears in the list.
    fn insert_idle(&self, entry: PrinterEntry) {
        if let Some(serial) = entry.serial.as_ref().map(|s| s.0.clone()) {
            let (shutdown, _rx) = watch::channel(false);
            let generation = self.next_generation();
            self.printers.lock().unwrap().insert(
                serial,
                PrinterHandle {
                    connection: ConnectionView::DISCONNECTED,
                    state: None,
                    shutdown,
                    generation,
                },
            );
        }
    }

    /// Stop a printer's supervisor, if any, without touching config or secrets.
    fn stop_supervisor(&self, serial: &str) {
        if let Some(handle) = self.printers.lock().unwrap().remove(serial) {
            let _ = handle.shutdown.send(true);
        }
    }

    /// Spawn a supervisor for one printer plus the task that forwards its events
    /// to the window.
    fn spawn_supervisor(&self, entry: PrinterEntry, access_code: String) {
        let Some(serial) = entry.serial.as_ref().map(|s| s.0.clone()) else {
            return;
        };
        let Some(address) = entry
            .last_address
            .as_ref()
            .and_then(|a| a.parse::<std::net::IpAddr>().ok())
        else {
            // No usable address yet — list it idle until discovery finds one.
            self.insert_idle(entry);
            return;
        };

        let endpoint = PrinterEndpoint::new(address, DeviceSerial(serial.clone()));
        let transport = TlsTransport::new(
            endpoint.clone(),
            Arc::clone(&self.pins) as Arc<dyn PinStore>,
        );
        let spec = SessionSpec {
            endpoint,
            credentials: Credentials::lan(access_code),
            config: SessionConfig::for_serial(&DeviceSerial(serial.clone())),
        };

        let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // A fresh generation stamps both the handle and its forwarder. If this
        // printer is later stopped and replaced (re-add, resolve-trust), the old
        // supervisor's forwarder — which may still emit a final `Disconnected`
        // or a late `Report` after shutdown is *signalled* but before its task
        // ends — will see a mismatched generation and drop the event instead of
        // corrupting its successor's handle.
        let generation = self.next_generation();
        self.printers.lock().unwrap().insert(
            serial.clone(),
            PrinterHandle {
                connection: ConnectionView::DISCONNECTED,
                state: None,
                shutdown: shutdown_tx,
                generation,
            },
        );

        // `tauri::async_runtime::spawn`, not `tokio::spawn`: `initialise` runs
        // inside Tauri's `setup`, where there is no ambient tokio runtime to
        // spawn onto. Tauri's runtime handle has one; the session's own tokio
        // primitives (timers, TCP, channels) then run on it.
        tauri::async_runtime::spawn(supervise(
            transport,
            spec,
            events_tx,
            shutdown_rx,
            Backoff::default(),
            EqualJitter,
        ));

        // The forwarder: fold each event into the handle and push a fresh view.
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = events_rx.recv().await {
                forward_event(&app, &serial, generation, event);
            }
        });
    }
}

/// Fold one session event into shared state and emit the appropriate Tauri
/// event. Runs on the forwarder task; it re-locks the managed state itself.
///
/// `generation` is the identity of the supervisor that owns this forwarder. If
/// the printer has since been replaced (re-add, resolve-trust) the handle under
/// this serial carries a newer generation, and every branch here becomes a
/// no-op — a superseded session can neither mutate nor emit for its successor.
fn forward_event(app: &AppHandle, serial: &str, generation: u64, event: SessionEvent) {
    use tauri::Manager;
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    match event {
        SessionEvent::State(connection) => {
            let view = ConnectionView::from_state(&connection);
            // Check generation and mutate under the same guard: no window for a
            // replacement to slip in between the ownership check and the write.
            let mut printers = state.printers.lock().unwrap();
            match printers.get_mut(serial) {
                Some(handle) if handle.generation == generation => handle.connection = view,
                _ => return,
            }
        }
        SessionEvent::Report(printer_state) => {
            let mut printers = state.printers.lock().unwrap();
            match printers.get_mut(serial) {
                Some(handle) if handle.generation == generation => {
                    handle.state = Some(*printer_state);
                }
                _ => return,
            }
        }
        SessionEvent::TrustDecisionRequired {
            pinned, presented, ..
        } => {
            // Only prompt if this forwarder still owns the printer, so a
            // superseded session cannot raise a trust dialog for a cert the
            // current session never saw.
            {
                let printers = state.printers.lock().unwrap();
                match printers.get(serial) {
                    Some(handle) if handle.generation == generation => {}
                    _ => return,
                }
            }
            let _ = app.emit(
                events::TRUST_REQUIRED,
                serde_json::json!({
                    "serial": serial,
                    "pinned": pinned.to_string(),
                    "presented": presented.to_string(),
                }),
            );
            return; // the view push below would just show Failed; the prompt is the point
        }
        // `SessionEvent` is `#[non_exhaustive]`; ignore a future event kind
        // rather than pushing a stale view for it.
        _ => return,
    }

    // Push the whole updated view for this printer — the UI replaces its card.
    if let Some(view) = state
        .printer_views()
        .into_iter()
        .find(|v| v.serial == serial)
    {
        let _ = app.emit(events::PRINTER_UPDATE, view);
    }
}

/// Adapts `pandaspy-store`'s standalone [`CertPinStore`] (`serial → [u8; 32]`)
/// to the client's [`PinStore`] trait (`serial → CertificateFingerprint`).
///
/// This adapter is why the two crates stay decoupled: `pandaspy-store` never
/// mentions `pandaspy-client`, and the translation lives here in the layer that
/// is allowed to know about both.
#[derive(Debug)]
pub struct PinStoreAdapter {
    inner: CertPinStore,
}

impl PinStoreAdapter {
    fn new(inner: CertPinStore) -> Self {
        Self { inner }
    }
}

impl PinStore for PinStoreAdapter {
    fn pinned(&self, serial: &DeviceSerial) -> Option<CertificateFingerprint> {
        self.inner.pinned(&serial.0).map(CertificateFingerprint)
    }

    fn pin(
        &self,
        serial: &DeviceSerial,
        fingerprint: CertificateFingerprint,
    ) -> Result<(), PinStoreError> {
        self.inner
            .pin(&serial.0, fingerprint.0)
            .map_err(|e| PinStoreError(e.to_string()))
    }
}

/// Choose the secret backend at runtime: the OS keyring when it answers, else
/// the encrypted-file fallback keyed to machine-bound material.
fn choose_secret_backend(base: &std::path::Path) -> (Arc<dyn SecretStore>, SecretBackend) {
    if keyring_available() {
        (Arc::new(KeyringSecrets::new()), SecretBackend::OsKeyring)
    } else {
        let store = EncryptedFileSecrets::new(machine_key_material(), base.join("secrets.enc"));
        (Arc::new(store), SecretBackend::EncryptedFile)
    }
}

/// Stable, machine-bound key material for the encrypted-file fallback.
///
/// This path is only reached on a host with no OS keyring — in practice a
/// headless/minimal Linux session — so the Linux machine-id is the natural
/// source. Reading it is genuinely platform-specific, which is exactly what
/// `src-tauri` is allowed to do; everywhere else we fall back to the hostname
/// so the function is total. (On macOS/Windows the keyring is always present,
/// so this material is effectively never used.)
fn machine_key_material() -> zeroize::Zeroizing<Vec<u8>> {
    #[cfg(target_os = "linux")]
    {
        for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
            if let Ok(id) = std::fs::read(path) {
                if !id.is_empty() {
                    return zeroize::Zeroizing::new(id);
                }
            }
        }
    }
    // Last resort, and honest about being weaker: the hostname is not secret,
    // but combined with the encrypted file's per-write salt it still keeps the
    // codes off the disk in plaintext. The threat model in
    // `EncryptedFileSecrets` documents the limits.
    let host = hostname_bytes();
    zeroize::Zeroizing::new(host)
}

fn hostname_bytes() -> Vec<u8> {
    std::env::var_os("HOSTNAME")
        .or_else(|| std::env::var_os("COMPUTERNAME"))
        .map(|h| h.to_string_lossy().into_owned().into_bytes())
        .unwrap_or_else(|| b"pandaspy-fallback-key".to_vec())
}

/// The per-user config directory, created if missing.
///
/// TODO(scaffold): the directory should be created `0600`/owner-only so the
/// encrypted-file fallback's "other local users" guarantee holds — that needs
/// per-OS permission code, which belongs here in `src-tauri`.
fn config_dir(app: &AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("pandaspy"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // The PinStoreAdapter is the one piece of app.rs testable without a running
    // Tauri app: it is pure translation over CertPinStore.
    #[test]
    fn the_pin_adapter_round_trips_through_the_store() {
        let dir = std::env::temp_dir().join(format!("pandaspy-pintest-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let adapter = PinStoreAdapter::new(CertPinStore::new(dir.join("pins.json")));
        let serial = DeviceSerial("00M09A000000000".to_owned());
        let fp = CertificateFingerprint([7; 32]);

        assert_eq!(adapter.pinned(&serial), None);
        adapter.pin(&serial, fp).unwrap();
        assert_eq!(adapter.pinned(&serial), Some(fp));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn machine_key_material_is_never_empty() {
        // A total function: even with no machine-id and no hostname env, it
        // yields usable key material rather than an empty (guessable) key.
        assert!(!machine_key_material().is_empty());
    }

    // Guards that the events map is what ipc.ts expects; a rename here without
    // one there would silently break the bridge.
    #[test]
    fn event_names_are_stable_and_distinct() {
        let mut seen = HashSet::new();
        for name in [events::PRINTER_UPDATE, events::TRUST_REQUIRED] {
            assert!(name.starts_with("printer://"));
            assert!(seen.insert(name), "duplicate event name: {name}");
        }
    }
}
