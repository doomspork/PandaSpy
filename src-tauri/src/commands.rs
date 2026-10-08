//! The Tauri command surface: the request/response half of the frontend
//! contract. Events (the push half) live in [`crate::app::events`].
//!
//! Every command is thin — it validates, delegates to [`AppState`], and maps the
//! result into a `camelCase` view. No protocol logic, no I/O beyond what the
//! domain crates already own. `src/lib/ipc.ts` mirrors these signatures.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use pandaspy_discovery::{
    DiscoveredPrinter, DiscoveryOptions, DiscoverySource, DiscoveryVerdict, discover, net,
};
use pandaspy_store::{SecretBackend, os_keyring_name, read_studio_config};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::app::AppState;
use crate::i18n::Localiser;
use crate::view::PrinterView;

/// List every configured printer with its current connection and state. The UI
/// calls this once on mount, then lives off `printer://update` events.
#[tauri::command]
pub fn list_printers(state: State<'_, AppState>) -> Vec<PrinterView> {
    state.printer_views()
}

/// A discovered printer, cleaned up for the add-printer picker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredView {
    pub serial: Option<String>,
    pub address: String,
    /// The human model name (`X1 Carbon`), never the raw product code.
    pub model: Option<String>,
    pub name: Option<String>,
    /// `ssdp` / `subnet-probe` / `manual`.
    pub source: &'static str,
    /// `true` when the printer is already in the user's list.
    pub already_added: bool,
}

/// What a discovery run tells the UI: the findings plus a verdict so "nothing
/// found" comes with an explanation rather than an empty void.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryReport {
    pub printers: Vec<DiscoveredView>,
    pub verdict: DiscoveryVerdict,
}

/// Scan the local network for printers (SSDP, with a subnet-probe fallback).
///
/// `async` because it awaits sockets; Tauri runs it on its own runtime, so it
/// does not block the UI thread.
#[tauri::command]
pub async fn discover_printers(state: State<'_, AppState>) -> Result<DiscoveryReport, String> {
    let known = state.known_serials();

    let stack = net::TokioSsdpStack::new();
    let probe = Arc::new(net::TlsCertProbe::new());
    let interfaces = net::SystemInterfaces;
    let outcome = discover(&stack, probe, &interfaces, &DiscoveryOptions::default()).await;

    let printers = outcome
        .printers
        .into_iter()
        .map(|p| DiscoveredView::from_discovered(p, &known))
        .collect();

    Ok(DiscoveryReport {
        printers,
        verdict: outcome.verdict,
    })
}

impl DiscoveredView {
    fn from_discovered(p: DiscoveredPrinter, known: &[String]) -> Self {
        let serial = p.serial.map(|s| s.0);
        let already_added = serial.as_ref().is_some_and(|s| known.contains(s));
        Self {
            serial,
            address: p.address.to_string(),
            model: p.model.map(|m| m.display_name().to_owned()),
            name: p.name,
            source: match p.source {
                DiscoverySource::Ssdp => "ssdp",
                DiscoverySource::SubnetProbe => "subnet-probe",
                DiscoverySource::Manual => "manual",
                // `DiscoverySource` is `#[non_exhaustive]`.
                _ => "unknown",
            },
            already_added,
        }
    }
}

/// Add a printer and bring its session up. `accessCode` is written to the secret
/// store and never round-trips back to the UI.
#[tauri::command]
pub fn add_printer(
    state: State<'_, AppState>,
    serial: String,
    address: String,
    access_code: String,
    nickname: Option<String>,
) -> Result<(), String> {
    if serial.trim().is_empty() {
        return Err("serial must not be empty".to_owned());
    }
    if access_code.trim().is_empty() {
        return Err("access code must not be empty".to_owned());
    }
    // Reject a non-IP address here rather than persisting it: the session layer
    // can only dial an `IpAddr` and would otherwise list the printer as a
    // permanently disconnected card with no explanation. A blank/hostname entry
    // fails fast and visibly instead. (Discovery-sourced adds always carry a
    // real IP, so this only bites manual entry.)
    let address = address.trim().to_owned();
    if address.parse::<std::net::IpAddr>().is_err() {
        return Err("address must be a valid IP address".to_owned());
    }
    state.add_printer(serial, address, access_code, normalise(nickname))
}

/// Remove a printer entirely: stop its session, forget its secret and pin.
#[tauri::command]
pub fn remove_printer(state: State<'_, AppState>, serial: String) -> Result<(), String> {
    state.remove_printer(&serial)
}

/// Reorder the printer list to match the given serials, top to bottom.
#[tauri::command]
pub fn reorder_printers(state: State<'_, AppState>, serials: Vec<String>) -> Result<(), String> {
    state.reorder(&serials)
}

/// Respond to a `printer://trust-required` prompt. `accept` re-pins the new
/// certificate and reconnects; declining leaves the printer stopped.
#[tauri::command]
pub fn resolve_trust(
    state: State<'_, AppState>,
    serial: String,
    accept: bool,
) -> Result<(), String> {
    state.resolve_trust(&serial, accept)
}

/// The user-facing settings, including where secrets actually live.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    /// `None` means "follow the operating system".
    pub locale: Option<String>,
    pub launch_at_login: bool,
    /// `os-keyring` / `encrypted-file` — the honest answer to "where is my
    /// access code stored?", which differs per host.
    pub secret_backend: &'static str,
    /// The platform's product name for its keyring (`Keychain`, …), so the UI
    /// can say exactly where the code went.
    pub keyring_name: &'static str,
}

/// Read the current settings.
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> SettingsView {
    let (locale, launch_at_login) = state.settings();
    SettingsView {
        locale,
        launch_at_login,
        secret_backend: backend_key(state.secret_backend()),
        keyring_name: os_keyring_name(),
    }
}

/// Persist settings. `locale = None` restores "follow the OS"; the change takes
/// effect immediately for freshly emitted views and applies launch-at-login via
/// the autostart plugin.
#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    localiser: State<'_, Localiser>,
    locale: Option<String>,
    launch_at_login: bool,
) -> Result<(), String> {
    let locale = locale.filter(|l| !l.trim().is_empty());
    state.set_settings(locale.clone(), launch_at_login)?;

    // Resolve the effective locale for HMS/error text the same way startup does,
    // so "follow the OS" and a bare `pl` both land on a real embedded locale
    // (`pl-PL`) rather than falling through to English.
    let preferences = match &locale {
        Some(tag) => vec![tag.clone()],
        None => sys_locale::get_locales().collect(),
    };
    state.set_lang(localiser.negotiate(&preferences));

    apply_autostart(&app, launch_at_login);
    Ok(())
}

/// A printer Bambu Studio already knows about, offered for one-tap import.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioPrinterView {
    pub serial: String,
    /// From discovery — Studio's config does not name printers.
    pub name: Option<String>,
    /// The human model name, from discovery.
    pub model: Option<String>,
    /// Where the printer is now (discovery), else where Studio last saw it.
    pub address: Option<String>,
    /// Whether Studio holds an access code for it. The code itself never
    /// crosses to the UI; [`add_studio_printer`] reads it again on this side.
    pub has_access_code: bool,
    pub already_added: bool,
}

/// List the printers configured in Bambu Studio.
///
/// Studio records an IP only for printers added by address and finds the rest
/// over SSDP at runtime, so most entries arrive without one. A discovery run
/// fills those in by serial — and refreshes a stale recorded address, since a
/// DHCP lease can have moved since Studio last wrote its config.
#[tauri::command]
pub async fn import_studio(
    app: AppHandle,
    state: State<'_, AppState>,
    import: State<'_, StudioImport>,
) -> Result<Vec<StudioPrinterView>, String> {
    let studio = read_studio_config(&studio_config_path(&app)?).map_err(|e| e.to_string())?;
    if studio.is_empty() {
        import.0.lock().unwrap().clear();
        return Ok(Vec::new());
    }

    let stack = net::TokioSsdpStack::new();
    let probe = Arc::new(net::TlsCertProbe::new());
    let interfaces = net::SystemInterfaces;
    let found = discover(&stack, probe, &interfaces, &DiscoveryOptions::default())
        .await
        .printers;

    let known = state.known_serials();
    let mut resolved = HashMap::new();
    let views = studio
        .into_iter()
        .map(|printer| {
            let seen = found
                .iter()
                .find(|d| d.serial.as_ref() == Some(&printer.serial));
            let address = seen.map(|d| d.address).or(printer.address);
            if let Some(address) = address {
                resolved.insert(printer.serial.0.clone(), address);
            }
            StudioPrinterView {
                name: seen.and_then(|d| d.name.clone()),
                model: seen
                    .and_then(|d| d.model.as_ref())
                    .map(|m| m.display_name().to_owned()),
                address: address.map(|a| a.to_string()),
                has_access_code: printer.access_code.is_some(),
                already_added: known.contains(&printer.serial.0),
                serial: printer.serial.0,
            }
        })
        .collect();
    *import.0.lock().unwrap() = resolved;
    Ok(views)
}

/// Where the last [`import_studio`] run placed each Studio printer, by serial.
///
/// [`add_studio_printer`] dials only an address recorded here, never one the
/// webview supplies. The access code is Studio's, not the user's typing, so
/// the backend — not the caller — has to decide where it is sent; otherwise
/// anything able to invoke commands could pair a real serial's code with an
/// address of its choosing and have TOFU pin that endpoint on first use.
#[derive(Default)]
pub struct StudioImport(Mutex<HashMap<String, IpAddr>>);

/// Add a printer using the access code Bambu Studio holds for it, so the user
/// does not have to look it up on the printer's screen.
///
/// The address is the one the preceding [`import_studio`] resolved; a printer
/// it could not place has to go through the manual form instead.
#[tauri::command]
pub fn add_studio_printer(
    app: AppHandle,
    state: State<'_, AppState>,
    import: State<'_, StudioImport>,
    serial: String,
    nickname: Option<String>,
) -> Result<(), String> {
    let address = import
        .0
        .lock()
        .unwrap()
        .get(&serial)
        .copied()
        .ok_or_else(|| format!("{serial} has no known address; import from Bambu Studio again"))?;
    let access_code = read_studio_config(&studio_config_path(&app)?)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|p| p.serial.0 == serial)
        .and_then(|p| p.access_code)
        .ok_or_else(|| format!("Bambu Studio has no access code for {serial}"))?;
    add_printer(state, serial, address.to_string(), access_code, nickname)
}

/// `BambuStudio.conf` in Studio's own config directory. The platform's config
/// directory is where Studio puts it on all three: `~/Library/Application
/// Support`, `%APPDATA%` (roaming) and `~/.config`.
fn studio_config_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .config_dir()
        .map_err(|e| e.to_string())?
        .join("BambuStudio")
        .join("BambuStudio.conf"))
}

fn backend_key(backend: SecretBackend) -> &'static str {
    match backend {
        SecretBackend::OsKeyring => "os-keyring",
        SecretBackend::EncryptedFile => "encrypted-file",
        // `SecretBackend` is `#[non_exhaustive]`.
        _ => "unknown",
    }
}

/// Turn empty/whitespace nicknames into `None` so the UI's model fallback kicks
/// in rather than showing a blank name.
fn normalise(nickname: Option<String>) -> Option<String> {
    nickname
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Enable or disable launch-at-login, best effort. A failure here must not fail
/// the settings save — the preference is still recorded, and the log says why
/// the OS side did not take.
fn apply_autostart(app: &AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    if let Err(error) = result {
        eprintln!("[autostart] could not set launch-at-login to {enabled}: {error}");
    }
}
