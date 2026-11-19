//! "Open at login", done with a per-user LaunchAgent. Turning it on writes the
//! agent's plist, and turning it off removes that file.

use std::fs;
use std::path::{Path, PathBuf};

const LABEL: &str = "au.com.lateralblast.mao";

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

/// Whether mao is set to open at login.
pub fn is_enabled() -> bool {
    agent_path().is_some_and(|path| path.exists())
}

/// Whether the toggle can be used: it needs the installed app, not a build directory.
pub fn is_available() -> bool {
    app_bundle().is_some()
}

/// Turns "open at login" on or off. Turning it on needs the installed app.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let path = agent_path().ok_or("HOME is not set")?;
    if !enabled {
        return match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("could not remove {}: {e}", path.display())),
        };
    }
    let app = app_bundle().ok_or("open at login needs the installed app; build it with npm run build")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
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
