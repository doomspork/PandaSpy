//! The menu bar / system tray icon.
//!
//! Built in Rust rather than declared in `tauri.conf.json` because the labels
//! are translated and the translations live in `locales/`. Two sources of truth
//! for one tray would be one too many.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{App, Manager, Runtime};

use crate::i18n::Localiser;

/// Menu item ids. Constants so the builder and the event handler cannot drift
/// apart over a typo.
const ID_SHOW: &str = "show";
const ID_QUIT: &str = "quit";

/// The tray's id, for [`tauri::Manager::tray_by_id`].
pub const TRAY_ID: &str = "main";

/// Install the tray icon and its menu.
///
/// Returns the handle. Hold onto it: Tauri's resource table keeps the icon
/// alive, but the handle is how you rebuild the menu when the user changes
/// language, and how you swap the icon to reflect print state later.
///
/// # Errors
///
/// If the platform refuses to create a tray icon. On Linux that usually means
/// no StatusNotifierItem host is running — i.e. `libayatana-appindicator3` is
/// missing, or the desktop environment has no tray at all.
pub fn install<R: Runtime>(
    app: &App<R>,
    localiser: &Localiser,
) -> Result<TrayIcon<R>, Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, ID_SHOW, localiser.get("tray-show"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, ID_QUIT, localiser.get("tray-quit"), true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    // Only `None` if `bundle.icon` in tauri.conf.json is empty or unreadable,
    // which is a packaging mistake rather than a runtime one — so it is worth
    // failing startup over rather than showing a blank square forever.
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("no default window icon; check `bundle.icon` in tauri.conf.json")?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        // TODO(scaffold): on macOS this should be a monochrome template image
        // so the menu bar can tint it for light and dark. The colour app icon
        // is a placeholder that at least looks right on Windows and Linux.
        .menu(&menu)
        // Left-click opens the menu rather than toggling the window. On a tray
        // app the menu is the primary surface, and a click that sometimes
        // shows a window and sometimes does not is worse than one that never
        // does.
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            ID_SHOW => show_main_window(app),
            ID_QUIT => app.exit(0),
            other => eprintln!("[tray] unhandled menu item: {other}"),
        })
        .build(app)
        .map_err(Into::into)
}

fn show_main_window<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        eprintln!("[tray] no window labelled `main` to show");
        return;
    };

    if let Err(error) = window.show().and_then(|()| window.set_focus()) {
        eprintln!("[tray] could not focus the main window: {error}");
    }
}
