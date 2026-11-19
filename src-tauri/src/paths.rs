//! Where the bundled helpers live, both in a built app and when run from the repo.

use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[cfg(target_arch = "aarch64")]
const TRIPLE: &str = "aarch64-apple-darwin";
#[cfg(not(target_arch = "aarch64"))]
const TRIPLE: &str = "x86_64-apple-darwin";

/// The Swift system-audio helper. Tauri places it next to the app binary when bundled.
pub fn tap_binary() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("mao-tap"));
        }
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries").join(format!("mao-tap-{TRIPLE}")));
    candidates.into_iter().find(|path| path.exists())
}

/// The Python script that discovers and streams to receivers.
pub fn stream_script(app: &AppHandle) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(dir) = app.path().resource_dir() {
        candidates.push(dir.join("python").join("mao_stream.py"));
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("python").join("mao_stream.py"));
    candidates.into_iter().find(|path| path.exists())
}

/// The Python interpreter from the mao virtual environment, or `MAO_PYTHON` if set.
pub fn python() -> PathBuf {
    if let Some(path) = std::env::var_os("MAO_PYTHON") {
        return PathBuf::from(path);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Library/Application Support/au.com.lateralblast.mao/venv/bin/python")
}
