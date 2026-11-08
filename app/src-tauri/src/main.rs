//! RoastMCP desktop app: starts (or finds) the daemon, shows its UI in a
//! native window and adds what a browser tab cannot: a system-wide emergency
//! stop shortcut and a guarded quit.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod daemon;

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use daemon::Daemon;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, RunEvent, Url, WindowEvent, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// The system-wide emergency stop. It works while another app has focus.
#[cfg(target_os = "macos")]
const ESTOP_SHORTCUT: &str = "Cmd+Shift+Escape";
#[cfg(not(target_os = "macos"))]
const ESTOP_SHORTCUT: &str = "Ctrl+Alt+Escape";

#[derive(Default)]
struct Shared {
    daemon: Mutex<Option<Daemon>>,
    /// Set once quitting is decided; window close and exit then pass.
    quitting: AtomicBool,
    /// A quit question is open; a second Cmd-Q does not ask twice.
    asking: AtomicBool,
}

fn german() -> bool {
    sys_locale::get_locale().is_some_and(|l| l.to_lowercase().starts_with("de"))
}

fn text(en: &'static str, de: &'static str) -> &'static str {
    if german() { de } else { en }
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        emergency_stop(app.clone());
                    }
                })
                .build(),
        )
        .manage(Shared::default())
        .menu(menu)
        .on_menu_event(|app, event| {
            if event.id() == "quit" {
                request_quit(app.clone());
            } else if event.id() == "show_log" {
                show_log();
            }
        })
        .setup(|app| {
            if let Err(e) = app.global_shortcut().register(ESTOP_SHORTCUT) {
                eprintln!("emergency stop shortcut {ESTOP_SHORTCUT} not available: {e}");
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || start(&handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
                && !window.state::<Shared>().quitting.load(Ordering::SeqCst)
            {
                api.prevent_close();
                request_quit(window.app_handle().clone());
            }
        })
        .build(tauri::generate_context!());

    let app = match app {
        Ok(app) => app,
        Err(e) => {
            eprintln!("RoastMCP could not start: {e}");
            std::process::exit(1);
        }
    };
    app.run(|app, event| match event {
        RunEvent::ExitRequested { api, .. }
            if !app.state::<Shared>().quitting.load(Ordering::SeqCst) =>
        {
            api.prevent_exit();
            request_quit(app.clone());
        }
        // macOS can end the app without asking (Dock, logout, `quit app`):
        // stop a daemon this app started, which leaves the roaster safe.
        RunEvent::Exit => {
            app.state::<Shared>().quitting.store(true, Ordering::SeqCst);
            let daemon = lock(app).take();
            if let Some(d) = daemon {
                d.shutdown();
            }
        }
        _ => {}
    });
}

/// The menu bar. Quit is our own item so Cmd-Q can ask while a roast runs.
fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let sep = || PredefinedMenuItem::separator(app);
    let quit = MenuItem::with_id(
        app,
        "quit",
        text("Quit RoastMCP", "RoastMCP beenden"),
        true,
        Some("CmdOrCtrl+Q"),
    )?;
    let app_menu = Submenu::with_items(
        app,
        "RoastMCP",
        true,
        &[
            &PredefinedMenuItem::about(app, None, None)?,
            &sep()?,
            &PredefinedMenuItem::services(app, None)?,
            &sep()?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &sep()?,
            &quit,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        text("Edit", "Bearbeiten"),
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &sep()?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        text("View", "Darstellung"),
        true,
        &[&PredefinedMenuItem::fullscreen(app, None)?],
    )?;
    let window = Submenu::with_items(
        app,
        text("Window", "Fenster"),
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &sep()?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    let show_log = MenuItem::with_id(
        app,
        "show_log",
        text("Show Log", "Protokoll anzeigen"),
        true,
        None::<&str>,
    )?;
    let help = Submenu::with_items(app, text("Help", "Hilfe"), true, &[&show_log])?;
    Menu::with_items(app, &[&app_menu, &edit, &view, &window, &help])
}

/// Reveals the daemon log in the file manager.
fn show_log() {
    let log = daemon::log_path();
    let (program, args): (&str, Vec<std::ffi::OsString>) = if cfg!(target_os = "macos") {
        ("open", vec!["-R".into(), log.into_os_string()])
    } else if cfg!(target_os = "windows") {
        (
            "explorer",
            vec![log.parent().map_or_else(
                || std::ffi::OsString::from("."),
                |p| p.as_os_str().to_owned(),
            )],
        )
    } else {
        (
            "xdg-open",
            vec![log.parent().map_or_else(
                || std::ffi::OsString::from("."),
                |p| p.as_os_str().to_owned(),
            )],
        )
    };
    let _ = std::process::Command::new(program).args(args).spawn();
}

/// The daemon binary, next to the app's own executable (Tauri puts the
/// sidecar there, in the bundle and in development).
fn sidecar_path() -> PathBuf {
    let name = format!("roastmcp{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(&name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// The built web UI: bundled as a resource, or `app/dist` in development.
fn ui_dir(app: &AppHandle) -> PathBuf {
    let bundled = app.path().resource_dir().map(|d| d.join("ui"));
    match bundled {
        Ok(d) if d.join("index.html").exists() => d,
        _ if cfg!(debug_assertions) => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist"),
        Ok(d) => d,
        Err(_) => PathBuf::from("ui"),
    }
}

/// During development the UI can come from the Vite dev server instead
/// (`ROASTMCP_UI_URL=http://localhost:1420`), for hot reload.
fn ui_base() -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var("ROASTMCP_UI_URL")
            .ok()
            .filter(|s| !s.is_empty())
    } else {
        None
    }
}

/// Starts or finds the daemon, then shows its UI; keeps watching a daemon the
/// app started and restarts it if it dies.
fn start(app: &AppHandle) {
    let sidecar = sidecar_path();
    let ui = ui_dir(app);
    loop {
        match Daemon::ensure(&sidecar, &ui) {
            Ok(d) => {
                let url = d.ui_url(ui_base().as_deref());
                let owned = d.owned();
                *lock(app) = Some(d);
                navigate(app, &url);
                if !owned {
                    return;
                }
            }
            Err(e) => {
                show_error(
                    app,
                    text(
                        "The roaster service could not be started.",
                        "Der Röst-Dienst konnte nicht gestartet werden.",
                    ),
                    &e,
                );
                return;
            }
        }
        // Watch the daemon this app started.
        loop {
            std::thread::sleep(Duration::from_secs(2));
            if app.state::<Shared>().quitting.load(Ordering::SeqCst) {
                return;
            }
            let mut guard = lock(app);
            let Some(d) = guard.as_mut() else { return };
            if d.exited() {
                *guard = None;
                eprintln!(
                    "the daemon exited unexpectedly; restarting (log: {})",
                    daemon::log_path().display()
                );
                break;
            }
        }
    }
}

fn lock(app: &AppHandle) -> std::sync::MutexGuard<'_, Option<Daemon>> {
    app.state::<Shared>()
        .inner()
        .daemon
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn navigate(app: &AppHandle, url: &str) {
    let (Some(win), Ok(url)) = (app.get_webview_window("main"), Url::parse(url)) else {
        return;
    };
    if let Err(e) = win.navigate(url) {
        eprintln!("cannot show the UI: {e}");
    }
}

fn show_error(app: &AppHandle, title: &str, detail: &str) {
    if let Some(win) = app.get_webview_window("main") {
        let js = format!(
            "window.showError && window.showError({}, {})",
            serde_json::Value::from(title),
            serde_json::Value::from(detail)
        );
        let _ = win.eval(js);
    }
}

fn show_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// The global shortcut: straight to the daemon, independent of the web view.
fn emergency_stop(app: AppHandle) {
    std::thread::spawn(move || {
        let result = lock(&app)
            .as_ref()
            .map(|d| d.emergency_stop("global shortcut"));
        match result {
            Some(Ok(())) => {}
            Some(Err(e)) => eprintln!("emergency stop failed: {e}"),
            None => eprintln!("emergency stop: no daemon"),
        }
        show_main(&app);
    });
}

/// Quits, after asking when a roast is running on a daemon this app started
/// (quitting stops that daemon, which puts the roaster into the safe state).
fn request_quit(app: AppHandle) {
    let shared = app.state::<Shared>();
    if shared.asking.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let busy = lock(&app)
            .as_ref()
            .filter(|d| d.owned())
            .map(Daemon::activity)
            .is_some_and(|a| a.recording || a.burner_on);
        let confirmed = !busy || {
            show_main(&app);
            app.dialog()
                .message(text(
                    "A roast is running. Quitting puts the roaster into the safe state \
                     (burner off, full air) and saves the recording.",
                    "Eine Röstung läuft. Beim Beenden geht der Röster in den sicheren Zustand \
                     (Brenner aus, volle Luft) und die Aufnahme wird gespeichert.",
                ))
                .title(text("Quit RoastMCP?", "RoastMCP beenden?"))
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    text("Quit", "Beenden").to_owned(),
                    text("Keep roasting", "Weiter rösten").to_owned(),
                ))
                .blocking_show()
        };
        let shared = app.state::<Shared>();
        if !confirmed {
            shared.asking.store(false, Ordering::SeqCst);
            return;
        }
        shared.quitting.store(true, Ordering::SeqCst);
        let daemon = lock(&app).take();
        if let Some(d) = daemon {
            d.shutdown();
        }
        app.exit(0);
    });
}
