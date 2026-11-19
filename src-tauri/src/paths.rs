//! Where the bundled helpers live, both in a built app and when run from the repo.

use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::{AppHandle, Manager};

#[cfg(target_arch = "aarch64")]
const TRIPLE: &str = "aarch64-apple-darwin";
#[cfg(not(target_arch = "aarch64"))]
const TRIPLE: &str = "x86_64-apple-darwin";

/// The Swift system-audio helper. Tauri places it next to the app binary when bundled.
pub fn tap_binary() -> Option<PathBuf> {
    helper("mao-tap")
}

/// How to run the streamer: the frozen binary that ships with the app, or, when developing
/// from the repo without one, the Python script in the mao environment.
#[derive(Clone, Debug)]
pub enum Streamer {
    Bundled(PathBuf),
    Script { python: PathBuf, script: PathBuf },
}

impl Streamer {
    /// The command to run the streamer with `args` appended.
    pub fn command(&self, args: &[String]) -> Command {
        match self {
            Streamer::Bundled(binary) => {
                let mut command = Command::new(binary);
                command.args(args);
                command
            }
            Streamer::Script { python, script } => {
                let mut command = Command::new(python);
                command.arg(script).args(args);
                command
            }
        }
    }
}

/// The streamer to use, or None when neither the bundled binary nor the script is found.
pub fn streamer(app: &AppHandle) -> Option<Streamer> {
    if let Some(binary) = helper("mao-stream") {
        return Some(Streamer::Bundled(binary));
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("python").join("mao_stream.py");
    let resource_script = app.path().resource_dir().ok().map(|dir| dir.join("python").join("mao_stream.py"));
    let script = resource_script.filter(|path| path.exists()).unwrap_or(script);
    script.exists().then(|| Streamer::Script {
        python: python(),
        script,
    })
}

/// The Python interpreter from the mao virtual environment, or `MAO_PYTHON` if set.
fn python() -> PathBuf {
    if let Some(path) = std::env::var_os("MAO_PYTHON") {
        return PathBuf::from(path);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Library/Application Support/au.com.lateralblast.mao/venv/bin/python")
}

/// A helper binary: next to the app binary when bundled, else the one built into the repo.
fn helper(name: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(name));
        }
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries").join(format!("{name}-{TRIPLE}")));
    candidates.into_iter().find(|path| path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_runs_through_the_python_interpreter() {
        let runner = Streamer::Script {
            python: PathBuf::from("/usr/bin/python3"),
            script: PathBuf::from("/x/mao_stream.py"),
        };
        let command = runner.command(&["scan".to_string()]);
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, vec!["/x/mao_stream.py", "scan"]);
    }

    #[test]
    fn bundled_binary_takes_the_arguments_directly() {
        let runner = Streamer::Bundled(PathBuf::from("/x/mao-stream"));
        let command = runner.command(&["stream".to_string(), "local:Speakers".to_string()]);
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, vec!["stream", "local:Speakers"]);
    }
}
