// Without this, launching the release build on Windows also opens a console
// window behind the app. Debug builds keep the console so `println!` works.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! PandaSpy — the platform layer.
//!
//! Deliberately thin. This crate owns the tray, the window, the Tauri commands
//! and the event bridge, and nothing else. Protocol parsing lives in
//! `pandaspy-proto`, finding printers in `pandaspy-discovery`, connections in
//! `pandaspy-client`, and persistence in `pandaspy-store`.
//!
//! Together with `pandaspy-store` this is one of the two places in the repository
//! permitted to use `#[cfg(target_os)]`. See `CLAUDE.md`.

mod app;
mod commands;
mod i18n;
mod tray;
mod view;

use serde::Serialize;
use tauri::{Manager, State};
use tauri_plugin_autostart::MacosLauncher;

use crate::app::AppState;
use crate::i18n::Localiser;

/// The updater plugin resolves its own crypto backend at compile time via the
/// `rustls-tls` feature, and that feature deliberately opts out of bundling a
/// default `CryptoProvider` — that opt-out is what keeps aws-lc-rs out of this
/// tree instead of pulling it in alongside ring. Nothing else in the process
/// installs a *default* provider (`pandaspy-client` and `pandaspy-discovery`
/// each build their own `rustls::ClientConfig` with an explicit
/// `Arc<CryptoProvider>`, so they never touch the global one), so without this
/// call the updater's first HTTPS request panics looking for one. Installing
/// ring here, not aws-lc-rs, keeps "rustls with the ring provider everywhere"
/// true for the update channel too. `install_default` returning `Err` just
/// means a provider is already installed — fine, and not our problem to
/// diagnose.
fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// What the app can tell you about itself.
///
/// Exists mainly so the placeholder frontend has a real command to call, and so
/// that "is the version single-sourcing actually working?" has an answer you
/// can read at runtime rather than infer from build config.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Diagnostics {
    /// Comes from `src-tauri/Cargo.toml` by way of Cargo, which is the same
    /// place `tauri.conf.json` inherits it from. One source, two readers.
    version: &'static str,
    active_locale: String,
    available_locales: Vec<String>,
    /// What the platform calls its secret store, for the settings UI.
    secret_backend: &'static str,
}

#[tauri::command]
fn diagnostics(localiser: State<'_, Localiser>) -> Diagnostics {
    Diagnostics {
        version: env!("CARGO_PKG_VERSION"),
        active_locale: localiser.active().to_owned(),
        available_locales: localiser.available(),
        secret_backend: pandaspy_store::os_keyring_name(),
    }
}

fn main() {
    install_crypto_provider();

    let mut localiser = Localiser::new();

    // `sys_locale` reads whatever the OS considers the user's preference. It is
    // the only thing we know at startup; a user override from the stored config
    // is applied later, once `pandaspy-store` can read it.
    let preferences: Vec<String> = sys_locale::get_locales().collect();
    let chosen = localiser.negotiate(&preferences);
    localiser.set_active(&chosen);

    tauri::Builder::default()
        // single-instance MUST be registered first: it decides whether this
        // process is the primary one before any other plugin or window spins
        // up. A second launch focuses the window we already have rather than
        // spawning a duplicate tray icon and a second set of printer sessions.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::reveal(app);
        }))
        // positioner anchors the popover to the tray icon. It has to see the
        // tray events (forwarded in `tray.rs`) to know where the icon is, so it
        // must be attached even though we drive it entirely from Rust.
        .plugin(tauri_plugin_positioner::init())
        // Launch-at-login. Registered so the settings UI can toggle it, but
        // DEFAULT OFF: we never call `enable()` here — see the setup hook.
        // `LaunchAgent` is the modern, sandbox-friendly macOS mechanism; the
        // `None` is "no extra args on autostart".
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        // OS notifications (print finished, printer error). Initialised now; the
        // code that raises them lands with the printer session, from Rust.
        .plugin(tauri_plugin_notification::init())
        // Self-update (M8). The frontend drives the whole flow — check on
        // launch and every 24h, prompt, download, install — through this
        // plugin's JS API; there is no Rust command to call it because none
        // is needed. `endpoints`/`pubkey` live in `tauri.conf.json` and are
        // still placeholders (see CLAUDE.md § Known gaps, "Updater is off"):
        // the endpoint 404s until a release publishes `latest.json`, which
        // itself stays gated behind signing secrets, so `check()` fails
        // closed rather than serving something unverifiable.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Lets the frontend call `relaunch()` after the updater installs, so
        // the new version starts without asking the user to reopen it.
        .plugin(tauri_plugin_process::init())
        // Every command the frontend can invoke. Implementations live in
        // `crate::commands`; `diagnostics` stays here as the self-describe hook.
        .invoke_handler(tauri::generate_handler![
            diagnostics,
            commands::list_printers,
            commands::discover_printers,
            commands::add_printer,
            commands::remove_printer,
            commands::reorder_printers,
            commands::resolve_trust,
            commands::get_settings,
            commands::set_settings,
            commands::import_studio,
        ])
        .setup(move |app| {
            // Captured by value; rebound `mut` so the stored-locale override
            // below can refine the active locale before the tray is built.
            let mut localiser = localiser;

            // A menu-bar app has no Dock presence — no Dock icon, no
            // app-switcher entry. Without this the scaffold bounces into the
            // Dock like an ordinary window app. macOS-only because the concept
            // does not exist elsewhere; this is a genuine per-platform branch,
            // which is exactly what src-tauri is allowed to have.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Shape the one window for this platform: a borderless popover on
            // macOS/Windows, a normal centred window on Linux. `tray.rs` owns
            // the reasoning for why Linux differs.
            tray::configure_window(app);

            // Load persisted state — printer list, chosen secret backend, pins.
            // The provisional locale is the OS-negotiated one set above; a stored
            // override from config wins over it, so resolve that before anything
            // reads a locale-dependent string.
            let state = AppState::load(app.handle().clone(), localiser.active().to_owned());
            if let Some(stored) = state.stored_locale() {
                let effective = localiser.negotiate(&[stored]);
                localiser.set_active(&effective);
                state.set_lang(effective);
            }

            // NB: autostart is applied from `set_settings` (the settings UI),
            // never forced on here — opting in is the user's choice.

            // Managed, not dropped: the tray handle is how the menu gets rebuilt
            // on a language change, and how the icon reflects print state. Build
            // it with the now-final locale.
            let tray = tray::install(app, &localiser)?;
            app.manage(tray);
            app.manage(localiser);

            // The Rust -> frontend event bridge. Rust owns ALL printer state —
            // discovery (`pandaspy-discovery`), the connection (`pandaspy-client`)
            // and the parsed status (`pandaspy-proto`) all live behind this
            // process. Sessions push `printer://update` events (see
            // `crate::app`); the window renders what it is told and NEVER opens a
            // socket to a printer itself.
            //
            // Order matters: `manage` first, `start` second. Each session spawns
            // a forwarder that resolves the state with `try_state`, which is
            // empty until `manage` has run.
            app.manage(state);
            app.state::<AppState>().start();

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start PandaSpy");
}
