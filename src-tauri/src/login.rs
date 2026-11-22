//! "Open at login". On macOS 13 and later this uses the login-item API (SMAppService), which shows
//! the item under Login Items in System Settings and does not need to write any files. On older
//! versions, or if the API refuses, it falls back to a per-user LaunchAgent: a plist written to
//! `~/Library/LaunchAgents`, which can fail if that folder is not owned by the user.

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr;

const LABEL: &str = "au.com.lateralblast.mao";
/// SMAppServiceStatusEnabled.
const STATUS_ENABLED: isize = 1;

// Loads the ServiceManagement framework, so that SMAppService is available to look up.
#[link(name = "ServiceManagement", kind = "framework")]
extern "C" {}

fn agent_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

/// The installed `.app` this process runs from, or None when run from a build directory.
fn app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // Contents/MacOS/mao sits three levels below the bundle.
    let bundle = exe.parent()?.parent()?.parent()?;
    (bundle.extension().is_some_and(|ext| ext == "app")).then(|| bundle.to_path_buf())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The LaunchAgent that opens `app` quietly at login.
pub fn agent_plist(app: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/bin/open</string>
    <string>-g</string>
    <string>{app}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
        app = escape(&app.to_string_lossy())
    )
}

/// The login-item service for this app, if this macOS has the API (SMAppService, 13 and later).
fn login_service() -> Option<*mut AnyObject> {
    let name = CString::new("SMAppService").ok()?;
    let class = AnyClass::get(&name)?;
    let service: *mut AnyObject = unsafe { msg_send![class, mainAppService] };
    (!service.is_null()).then_some(service)
}

/// Whether this macOS has the login-item API. Used to choose between it and the LaunchAgent.
pub fn uses_login_item_api() -> bool {
    CString::new("SMAppService").ok().and_then(|name| AnyClass::get(&name)).is_some()
}

fn service_enabled(service: *mut AnyObject) -> bool {
    let status: isize = unsafe { msg_send![service, status] };
    status == STATUS_ENABLED
}

fn set_service(service: *mut AnyObject, enabled: bool) -> Result<(), String> {
    let mut error: *mut AnyObject = ptr::null_mut();
    let accepted: bool = unsafe {
        if enabled {
            msg_send![service, registerAndReturnError: &mut error as *mut *mut AnyObject]
        } else {
            msg_send![service, unregisterAndReturnError: &mut error as *mut *mut AnyObject]
        }
    };
    if accepted {
        Ok(())
    } else {
        Err("macOS did not accept the login item".into())
    }
}

/// Whether mao is set to open at login.
pub fn is_enabled() -> bool {
    if let Some(service) = login_service() {
        if service_enabled(service) {
            return true;
        }
    }
    agent_path().is_some_and(|path| path.exists())
}

/// Whether the toggle can be used: it needs the installed app, not a build directory.
pub fn is_available() -> bool {
    app_bundle().is_some()
}

/// Turns "open at login" on or off. Turning it on needs the installed app.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if !enabled {
        let mut result = Ok(());
        if let Some(service) = login_service() {
            if service_enabled(service) {
                result = set_service(service, false);
            }
        }
        if let Some(path) = agent_path() {
            if let Err(e) = fs::remove_file(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    result = result.and(Err(format!("could not remove {}: {e}", path.display())));
                }
            }
        }
        return result;
    }

    let app = app_bundle().ok_or("open at login needs the installed app; build it with ./mao")?;
    if uses_login_item_api() {
        if let Some(service) = login_service() {
            if set_service(service, true).is_ok() {
                // The login item is in place, so an old LaunchAgent is no longer needed.
                if let Some(path) = agent_path() {
                    let _ = fs::remove_file(&path);
                }
                return Ok(());
            }
        }
    }

    // Older macOS, or the login-item API refused: use the LaunchAgent.
    let path = agent_path().ok_or("HOME is not set")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("could not use {}: {e}", dir.display()))?;
    }
    fs::write(&path, agent_plist(&app)).map_err(|e| format!("could not write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_opens_the_app_quietly_at_login() {
        let plist = agent_plist(Path::new("/Applications/mao.app"));
        assert!(plist.contains("<string>au.com.lateralblast.mao</string>"));
        assert!(plist.contains("<string>/usr/bin/open</string>"));
        assert!(plist.contains("<string>/Applications/mao.app</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
    }

    #[test]
    fn plist_escapes_paths() {
        let plist = agent_plist(Path::new("/Users/a&b/<x>.app"));
        assert!(plist.contains("/Users/a&amp;b/&lt;x&gt;.app"));
    }
}
