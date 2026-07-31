//! The menu bar / system tray icon, and the window it drives.
//!
//! Built in Rust rather than declared in `tauri.conf.json` because the labels
//! are translated and the translations live in `locales/`. Two sources of truth
//! for one tray would be one too many.
//!
//! ## Why the platforms differ
//!
//! macOS and Windows get a borderless **popover** anchored to the tray icon:
//! click the icon and a small panel appears next to it. That is the native
//! idiom for a menu-bar / notification-area utility, and both OSes tell the app
//! where the icon was drawn, so `tauri-plugin-positioner` can place the panel.
//!
//! Linux does **not**. Tray hosts there speak StatusNotifierItem (AppIndicator
//! via libayatana): the icon is owned out-of-process and its screen position is
//! never reported back, and under Wayland a client cannot place a window at
//! absolute screen coordinates at all. A tray-anchored popover is therefore
//! impossible, not merely fiddly — so Linux gets the honest alternative: the
//! context menu is the primary surface, and "Show" opens a normal, centred,
//! decorated window.
//!
//! This asymmetry is a legitimate use of `#[cfg(target_os)]`: it is presentation
//! that genuinely differs between platforms, which is `src-tauri`'s job. No
//! protocol, discovery or state logic lives here. See `CLAUDE.md`.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{App, AppHandle, Manager, Runtime};

use crate::i18n::Localiser;

/// Menu item ids. Constants so the builder and the event handler cannot drift
/// apart over a typo.
const ID_SHOW: &str = "show";
const ID_QUIT: &str = "quit";

/// The tray's id, for [`tauri::Manager::tray_by_id`].
pub const TRAY_ID: &str = "main";

/// The single window, as labelled in `tauri.conf.json`.
const MAIN_WINDOW: &str = "main";

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

    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        // A plain "PandaSpy" — a proper noun, so it reads from the shared term
        // rather than being hardcoded here.
        .tooltip(localiser.get("window-title"))
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            // "Show" opens the popover on macOS/Windows, the centred window on
            // Linux; `reveal` picks the right one for the platform.
            ID_SHOW => reveal(app),
            ID_QUIT => app.exit(0),
            other => eprintln!("[tray] unhandled menu item: {other}"),
        });

    configure_click_behaviour(builder)
        .build(app)
        .map_err(Into::into)
}

/// Wire per-platform icon rendering and click behaviour onto the builder.
///
/// macOS: a monochrome **template** icon so the menu bar tints it for light and
/// dark, plus a left-click that toggles the tray-anchored popover.
#[cfg(target_os = "macos")]
fn configure_click_behaviour<R: Runtime>(builder: TrayIconBuilder<R>) -> TrayIconBuilder<R> {
    builder
        // A template image is monochrome by definition, so the menu bar can
        // tint it. That means print STATUS must be conveyed by the glyph's
        // SHAPE, never by colour.
        //
        // TODO(scaffold): swap the glyph per state (idle / printing / paused /
        // error) in a later milestone — distinct silhouettes, not distinct
        // colours. The template flag is wired now so that swap is a one-liner.
        .icon_as_template(true)
        // Left-click toggles the popover instead of the menu (the menu is on
        // right-click). A popover app's primary surface is the panel, not a
        // list of menu items.
        .show_menu_on_left_click(false)
        .on_tray_icon_event(handle_tray_click)
}

/// Windows: the colour icon is correct (the notification area is not tinted),
/// and a left-click toggles a popover near the tray.
#[cfg(target_os = "windows")]
fn configure_click_behaviour<R: Runtime>(builder: TrayIconBuilder<R>) -> TrayIconBuilder<R> {
    builder
        .show_menu_on_left_click(false)
        .on_tray_icon_event(handle_tray_click)
}

/// Linux: no popover is possible (see the module docs), so the menu IS the
/// surface — left-click opens it, and its "Show" item opens the centred window.
#[cfg(target_os = "linux")]
fn configure_click_behaviour<R: Runtime>(builder: TrayIconBuilder<R>) -> TrayIconBuilder<R> {
    builder.show_menu_on_left_click(true)
}

/// Handle a raw tray-icon click on macOS/Windows: record the icon's position
/// for the positioner, then toggle the popover on a left click.
#[cfg(not(target_os = "linux"))]
fn handle_tray_click<R: Runtime>(tray: &TrayIcon<R>, event: tauri::tray::TrayIconEvent) {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};

    // The positioner learns where the icon was drawn from every tray event, so
    // it must see this one BEFORE we ask it to move the window there.
    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);

    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        toggle_popover(tray.app_handle());
    }
}

/// Bring the window to the user. A tray-anchored popover on macOS/Windows, the
/// centred window on Linux. Shared by the "Show" menu item and the
/// single-instance second-launch handler.
pub fn reveal<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "linux")]
    show_window(app);
    #[cfg(not(target_os = "linux"))]
    show_popover(app);
}

/// Linux: show and focus the normal, centred window.
#[cfg(target_os = "linux")]
fn show_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        eprintln!("[tray] no window labelled `{MAIN_WINDOW}` to show");
        return;
    };

    if let Err(error) = window.show().and_then(|()| window.set_focus()) {
        eprintln!("[tray] could not focus the main window: {error}");
    }
}

/// macOS/Windows: anchor the popover to the tray icon, then show and focus it.
#[cfg(not(target_os = "linux"))]
fn show_popover<R: Runtime>(app: &AppHandle<R>) {
    use tauri_plugin_positioner::{Position, WindowExt};

    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        eprintln!("[tray] no window labelled `{MAIN_WINDOW}` to show");
        return;
    };

    // The menu bar sits at the top on macOS, so the popover drops down from the
    // icon; the notification area sits bottom-right on Windows, so it rises from
    // there. These anchors depend on the position the positioner recorded from
    // the last tray event — a "Show" triggered from the right-click menu with no
    // prior hover may fall back to a stale point.
    //
    // TODO(scaffold): tune the exact anchor per platform against real hardware
    // (menu-bar height, taskbar edge, multi-monitor) in a later milestone.
    #[cfg(target_os = "macos")]
    let anchor = Position::TrayCenter;
    #[cfg(target_os = "windows")]
    let anchor = Position::TrayBottomRight;

    if let Err(error) = window.move_window(anchor) {
        eprintln!("[tray] could not position the popover: {error}");
    }
    if let Err(error) = window.show().and_then(|()| window.set_focus()) {
        eprintln!("[tray] could not show the popover: {error}");
    }
}

/// macOS/Windows: a visible popover toggles shut; a hidden one re-anchors and
/// shows. This is what a tray-icon left-click does.
#[cfg(not(target_os = "linux"))]
fn toggle_popover<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        eprintln!("[tray] no window labelled `{MAIN_WINDOW}` to toggle");
        return;
    };

    match window.is_visible() {
        Ok(true) => {
            if let Err(error) = window.hide() {
                eprintln!("[tray] could not hide the popover: {error}");
            }
        }
        Ok(false) => show_popover(app),
        Err(error) => eprintln!("[tray] could not read popover visibility: {error}"),
    }
}

/// Shape the single window for this platform.
///
/// macOS/Windows: a borderless, floating, taskbar-less popover that dismisses
/// when it loses focus. Linux: left untouched — it stays the decorated, centred
/// window declared in `tauri.conf.json`, because it cannot be a popover.
#[cfg(not(target_os = "linux"))]
pub fn configure_window<R: Runtime>(app: &App<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        eprintln!("[tray] no window labelled `{MAIN_WINDOW}` to configure");
        return;
    };

    // A popover has no title bar, floats above other windows, is not resizable,
    // and does not appear in the taskbar/Dock switcher — it belongs to the tray
    // icon, not the window list. (`tauri.conf.json` already starts it hidden.)
    for result in [
        window.set_decorations(false),
        window.set_always_on_top(true),
        window.set_skip_taskbar(true),
        window.set_resizable(false),
    ] {
        if let Err(error) = result {
            eprintln!("[tray] could not shape the popover window: {error}");
        }
    }

    // Dismiss the popover when the user clicks away, the way a native one does.
    // `Focused(false)` fires when focus leaves the window.
    let dismiss = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(false) = event
            && let Err(error) = dismiss.hide()
        {
            eprintln!("[tray] could not hide the popover on blur: {error}");
        }
    });
}

/// Linux keeps the window exactly as declared in `tauri.conf.json` — decorated,
/// centred, listed in the taskbar — because a tray-anchored popover is not
/// possible there. See the module docs.
#[cfg(target_os = "linux")]
pub fn configure_window<R: Runtime>(_app: &App<R>) {}
