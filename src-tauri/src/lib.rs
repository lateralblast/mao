//! mao: a menu bar app that sends system audio to several AirPlay receivers at once.

mod controller;
mod devices;
mod login;
mod paths;
mod stream;

use controller::{Controller, Status};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Listener, Manager, PhysicalPosition, Rect, RunEvent, State, WindowEvent};

const POPOVER: &str = "main";
/// Mute or unmute everything: Option, Command and M.
const MUTE_ALL_SHORTCUT: &str = "alt+super+m";
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");
/// The same icon with a dot, shown while any receiver is streaming.
const TRAY_ICON_STREAMING: &[u8] = include_bytes!("../icons/tray-streaming.png");
/// Clicking the tray icon while the popover is open first hides it (focus is lost),
/// so ignore the reopen that would otherwise follow within this window.
const REOPEN_GUARD: Duration = Duration::from_millis(250);

#[derive(Default)]
struct Popover {
    hidden_at: Mutex<Option<Instant>>,
}

#[tauri::command]
fn get_status(controller: State<Arc<Controller>>) -> Status {
    controller.status()
}

#[tauri::command]
fn set_selected(identifiers: Vec<String>, app: AppHandle, controller: State<Arc<Controller>>) -> Result<Status, String> {
    controller.set_selected(&app, identifiers)
}

const README: &str = include_str!("../../README.md");
const HELP_HEADING: &str = "Help Support Development";

const ATTRIBUTION_HEADING: &str = "Attribution";

#[derive(serde::Serialize)]
struct About {
    version: &'static str,
    help_title: &'static str,
    help: String,
    attribution: String,
}

/// The lines under the README heading `heading`, up to the next heading.
fn readme_section(readme: &str, heading: &str) -> String {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in readme.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            if inside {
                break;
            }
            inside = title.trim() == heading;
            continue;
        }
        if inside {
            lines.push(line);
        }
    }
    lines.join("\n").trim().to_string()
}

#[tauri::command]
fn about() -> About {
    About {
        version: env!("CARGO_PKG_VERSION"),
        help_title: HELP_HEADING,
        help: readme_section(README, HELP_HEADING),
        attribution: readme_section(README, ATTRIBUTION_HEADING),
    }
}

/// Opens a web link in the default browser. Only http and https links are accepted.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only web links can be opened".into());
    }
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct LoginItem {
    enabled: bool,
    available: bool,
}

#[tauri::command]
fn login_item() -> LoginItem {
    LoginItem {
        enabled: login::is_enabled(),
        available: login::is_available(),
    }
}

#[tauri::command]
fn set_login_item(enabled: bool) -> Result<LoginItem, String> {
    login::set_enabled(enabled)?;
    Ok(login_item())
}

#[tauri::command]
fn get_notify_on_drop(controller: State<Arc<Controller>>) -> bool {
    controller.notify_on_drop()
}

#[tauri::command]
fn set_notify_on_drop(enabled: bool, controller: State<Arc<Controller>>) -> Result<bool, String> {
    controller.set_notify_on_drop(enabled)?;
    Ok(controller.notify_on_drop())
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn set_volume(identifier: String, level: f64, app: AppHandle, controller: State<Arc<Controller>>) -> Result<(), String> {
    controller.set_volume(&app, &identifier, level)
}

#[tauri::command]
fn set_mute(identifier: String, muted: bool, app: AppHandle, controller: State<Arc<Controller>>) -> Result<(), String> {
    controller.set_mute(&app, &identifier, muted)
}

#[tauri::command]
fn set_local_volume(identifier: String, level: f64, app: AppHandle, controller: State<Arc<Controller>>) -> Result<(), String> {
    controller.set_local_volume(&app, &identifier, level)
}

#[tauri::command]
fn set_local_mute(identifier: String, muted: bool, app: AppHandle, controller: State<Arc<Controller>>) -> Result<(), String> {
    controller.set_local_mute(&app, &identifier, muted)
}

#[tauri::command]
fn toggle_mute_all(app: AppHandle, controller: State<Arc<Controller>>) -> Result<bool, String> {
    controller.toggle_mute_all(&app)
}

#[tauri::command]
fn refresh_receivers(app: AppHandle, controller: State<Arc<Controller>>) {
    controller.refresh_scan(&app);
}

fn toggle_popover(app: &AppHandle, anchor: Rect) {
    let Some(window) = app.get_webview_window(POPOVER) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    let recently_hidden = app
        .state::<Popover>()
        .hidden_at
        .lock()
        .ok()
        .and_then(|at| *at)
        .is_some_and(|at| at.elapsed() < REOPEN_GUARD);
    if recently_hidden {
        return;
    }

    // Centre the popover under the tray icon.
    let scale = window.scale_factor().unwrap_or(1.0);
    let icon_pos = anchor.position.to_physical::<f64>(scale);
    let icon_size = anchor.size.to_physical::<f64>(scale);
    let width = window.outer_size().map(|s| s.width as f64).unwrap_or(320.0 * scale);
    let x = icon_pos.x + icon_size.width / 2.0 - width / 2.0;
    let y = icon_pos.y + icon_size.height;
    let _ = window.set_position(PhysicalPosition::new(x, y));
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit("popover-opened", ());
}

/// Shows the dot on the menu bar icon while any receiver is streaming.
fn update_tray_icon(app: &AppHandle, payload: &str) {
    let Ok(status) = serde_json::from_str::<serde_json::Value>(payload) else {
        return;
    };
    let streaming = status["receivers"]
        .as_array()
        .is_some_and(|list| list.iter().any(|r| r["state"] == "streaming"));
    let Some(tray) = app.tray_by_id("mao") else {
        return;
    };
    let bytes = if streaming { TRAY_ICON_STREAMING } else { TRAY_ICON };
    if let Ok(icon) = Image::from_bytes(bytes) {
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_icon_as_template(true);
    }
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Menu bar only: no Dock icon.
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    let config_dir = app.path().app_config_dir()?;
    let controller = Arc::new(Controller::load(config_dir.join("state.json")));
    app.manage(Arc::clone(&controller));

    let quit = MenuItem::with_id(app, "quit", "Quit mao", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&quit])?;

    TrayIconBuilder::with_id("mao")
        .icon(Image::from_bytes(TRAY_ICON)?)
        .icon_as_template(true)
        .tooltip("mao")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "quit" {
                app.exit(0);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle_popover(tray.app_handle(), rect);
            }
        })
        .build(app)?;

    // Resume the saved selection, and look for receivers on the network.
    let handle = app.handle().clone();
    let tray_handle = handle.clone();
    handle.listen(controller::STATUS_EVENT, move |event| {
        update_tray_icon(&tray_handle, event.payload());
    });
    controller.start_saved(&handle);
    controller.refresh_scan(&handle);

    // Re-read this Mac's outputs whenever one is plugged in or removed.
    let watched = Arc::clone(&controller);
    let watched_handle = handle.clone();
    if let Err(error) = devices::watch(move || {
        // Runs on a Core Audio thread, so hand the work to another thread.
        let controller = Arc::clone(&watched);
        let app = watched_handle.clone();
        std::thread::spawn(move || controller.refresh_local(&app));
    }) {
        eprintln!("mao: {error}");
    }

    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .manage(Popover::default())
        .invoke_handler(tauri::generate_handler![get_status, set_selected, refresh_receivers, quit_app, about, open_url, login_item, set_login_item, set_volume, set_mute, set_local_volume, set_local_mute, toggle_mute_all, get_notify_on_drop, set_notify_on_drop])
        .on_window_event(|window, event| {
            // The popover behaves like a menu: clicking elsewhere dismisses it.
            if let WindowEvent::Focused(false) = event {
                if window.label() == POPOVER {
                    let _ = window.hide();
                    if let Ok(mut at) = window.app_handle().state::<Popover>().hidden_at.lock() {
                        *at = Some(Instant::now());
                    }
                }
            }
        })
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcut(MUTE_ALL_SHORTCUT)
                .expect("the mute-all shortcut is valid")
                .with_handler(|app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        let controller: Arc<Controller> = app.state::<Arc<Controller>>().inner().clone();
                        let app = app.clone();
                        std::thread::spawn(move || {
                            let _ = controller.toggle_mute_all(&app);
                        });
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("failed to build mao")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                app.state::<Arc<Controller>>().shutdown();
            }
        });
}

#[cfg(test)]
mod help_tests {
    use super::*;

    #[test]
    fn help_section_is_read_from_the_readme() {
        let text = readme_section(README, HELP_HEADING);
        assert!(text.starts_with("If you find this software useful"), "unexpected start: {text:?}");
        assert!(text.contains("https://ko-fi.com/richardatlateralblast"));
        assert!(!text.contains("## "), "section should stop at the next heading");
    }

    #[test]
    fn attribution_is_read_from_the_readme() {
        let text = readme_section(README, ATTRIBUTION_HEADING);
        assert!(text.contains("MEDZ - Flaticon"), "unexpected attribution: {text:?}");
    }

    #[test]
    fn help_section_stops_at_the_next_heading() {
        let text = readme_section("## Help Support Development\n\nHello\n## Next\nNot this", HELP_HEADING);
        assert_eq!(text, "Hello");
    }
}

#[cfg(test)]
mod version_tests {
    use std::fs;
    use std::path::Path;

    fn repo_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("repo root")
    }

    fn read_json(path: &str) -> serde_json::Value {
        let text = fs::read_to_string(repo_root().join(path)).expect("read file");
        serde_json::from_str(&text).expect("parse JSON")
    }

    #[test]
    fn version_components_do_not_exceed_nine() {
        // The base conventions: MAJOR.MINOR.PATCH, and no component may exceed 9 (a patch past 9
        // rolls into the minor, and a minor past 9 into the major).
        let version = env!("CARGO_PKG_VERSION");
        let parts: Vec<u32> = version
            .split('.')
            .map(|part| part.parse().unwrap_or_else(|_| panic!("version part {part:?} is not a number")))
            .collect();
        assert_eq!(parts.len(), 3, "version {version} should be MAJOR.MINOR.PATCH");
        for part in &parts {
            assert!(*part <= 9, "version {version} has a component above 9");
        }
    }

    #[test]
    fn version_agrees_in_every_place() {
        let version = env!("CARGO_PKG_VERSION");

        assert_eq!(read_json("package.json")["version"], version, "package.json");
        assert_eq!(read_json("src-tauri/tauri.conf.json")["version"], version, "tauri.conf.json");

        let readme = fs::read_to_string(repo_root().join("README.md")).expect("read README.md");
        assert!(
            readme.lines().any(|line| line == format!("Version: {version}")),
            "README.md should contain a line `Version: {version}`"
        );

        let changelog = fs::read_to_string(repo_root().join("CHANGELOG.md")).expect("read CHANGELOG.md");
        let newest = changelog
            .lines()
            .find(|line| line.starts_with("## [") && !line.starts_with("## [Unreleased]"))
            .unwrap_or_default();
        assert!(
            newest.starts_with(&format!("## [{version}] - ")),
            "newest CHANGELOG heading should be for {version}, found {newest:?}"
        );
    }
}
