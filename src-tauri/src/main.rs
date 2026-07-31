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

mod i18n;
mod tray;

use serde::Serialize;
use tauri::{Manager, State};

use crate::i18n::Localiser;

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
    let mut localiser = Localiser::new();

    // `sys_locale` reads whatever the OS considers the user's preference. It is
    // the only thing we know at startup; a user override from the stored config
    // is applied later, once `pandaspy-store` can read it.
    let preferences: Vec<String> = sys_locale::get_locales().collect();
    let chosen = localiser.negotiate(&preferences);
    localiser.set_active(&chosen);

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![diagnostics])
        .setup(move |app| {
            // Managed, not dropped: the handle is how the tray menu gets
            // rebuilt when the user changes language, and how the icon gets
            // swapped to reflect print state.
            let tray = tray::install(app, &localiser)?;
            app.manage(tray);
            app.manage(localiser);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start PandaSpy");
}
