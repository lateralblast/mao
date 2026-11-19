//! mao: a menu bar app that sends system audio to several AirPlay receivers at once.

mod controller;
mod paths;
mod stream;

use controller::{Controller, Status};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Rect, RunEvent, State, WindowEvent};

const POPOVER: &str = "main";
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");
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
    controller.start_saved(&handle);
    controller.refresh_scan(&handle);

    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .manage(Popover::default())
        .invoke_handler(tauri::generate_handler![get_status, set_selected, refresh_receivers])
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
