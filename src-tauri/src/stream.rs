//! Runs the system-audio tap and the Python sidecar, and connects them with a pipe.

use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;

/// A receiver as reported by discovery.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Receiver {
    pub identifier: String,
    pub name: String,
    pub address: String,
}

/// A running tap and sidecar. Dropping it does not stop them; call `stop`.
pub struct Running {
    tap: Child,
    python: Child,
    // Keeping the tap's stdin open keeps it running. Closing it makes the tap exit.
    tap_stdin: Option<ChildStdin>,
}

impl Running {
    pub fn stop(mut self) {
        drop(self.tap_stdin.take());
        let _ = self.tap.kill();
        let _ = self.tap.wait();
        let _ = self.python.kill();
        let _ = self.python.wait();
    }
}

/// Starts streaming system audio to `receivers`. `on_line` gets each JSON line the
/// sidecar prints. `on_tap_exit` runs if the tap stops before we asked it to.
pub fn start(
    tap: &Path,
    python: &Path,
    script: &Path,
    receivers: &[Receiver],
    on_line: impl Fn(String) + Send + 'static,
    on_tap_exit: impl FnOnce() + Send + 'static,
) -> Result<Running, String> {
    let mut tap_child = Command::new(tap)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start the audio tap: {e}"))?;
    let tap_stdin = tap_child.stdin.take();
    let mut tap_out = tap_child.stdout.take().ok_or("no tap stdout")?;
    let tap_err = tap_child.stderr.take().ok_or("no tap stderr")?;

    let mut args: Vec<String> = vec!["stream".into()];
    args.extend(receivers.iter().map(target));
    let mut python_child = Command::new(python)
        .arg(script)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start the streamer ({}): {e}", python.display()))?;
    let mut python_in = python_child.stdin.take().ok_or("no streamer stdin")?;
    let python_out = python_child.stdout.take().ok_or("no streamer stdout")?;
    let python_err = python_child.stderr.take().ok_or("no streamer stderr")?;

    thread::spawn(move || {
        let _ = io::copy(&mut tap_out, &mut python_in);
        // Dropping python_in ends the streamer's input, so it finishes on its own.
        drop(python_in);
        on_tap_exit();
    });
    thread::spawn(move || {
        for line in BufReader::new(python_out).lines().map_while(Result::ok) {
            on_line(line);
        }
    });
    thread::spawn(move || log_lines("mao-stream", python_err));
    thread::spawn(move || log_lines("mao-tap", tap_err));

    Ok(Running {
        tap: tap_child,
        python: python_child,
        tap_stdin,
    })
}

fn log_lines(source: &str, reader: impl io::Read) {
    for line in BufReader::new(reader).lines().map_while(Result::ok) {
        eprintln!("[{source}] {line}");
    }
}

/// The argument the streamer expects for a receiver: `local:NAME`, or `IDENTIFIER=ADDRESS`.
fn target(receiver: &Receiver) -> String {
    if is_local(&receiver.identifier) {
        receiver.identifier.clone()
    } else {
        format!("{}={}", receiver.identifier, receiver.address)
    }
}

/// Whether an identifier names an output device on this Mac rather than an AirPlay receiver.
pub fn is_local(identifier: &str) -> bool {
    identifier.starts_with("local:")
}

/// Lists the AirPlay receivers on the network. Blocks for a few seconds.
pub fn scan(python: &Path, script: &Path) -> Result<Vec<Receiver>, String> {
    run_json(python, script, "scan")
}

/// Lists this Mac's output devices that can take a copy of the audio.
pub fn local_devices(python: &Path, script: &Path) -> Result<Vec<Receiver>, String> {
    run_json(python, script, "devices")
}

fn run_json(python: &Path, script: &Path, command: &str) -> Result<Vec<Receiver>, String> {
    let output = Command::new(python)
        .arg(script)
        .arg(command)
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("could not run the streamer ({}): {e}", python.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let last = stderr.lines().last().unwrap_or("no output");
        return Err(format!("discovery failed: {last}"));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("could not read discovery output: {e}"))
}

#[cfg(test)]
mod hardware_tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// Needs Kitchen on the network and audio capture permission.
    /// Run with `cargo test -- --ignored --nocapture streams_to_a_receiver`.
    #[test]
    #[ignore]
    fn streams_to_a_receiver() {
        let tap = crate::paths::tap_binary().expect("the tap binary is built");
        let python = crate::paths::python();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("python").join("mao_stream.py");
        let receiver = Receiver {
            identifier: "B2:5B:17:C7:B2:04".into(),
            name: "Kitchen".into(),
            address: "192.168.1.62".into(),
        };

        let (tx, rx) = mpsc::channel();
        let running = start(&tap, &python, &script, &[receiver], move |line| {
            let _ = tx.send(line);
        }, || {})
        .expect("start streaming");

        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Ok(line) = rx.recv_timeout(Duration::from_millis(500)) {
                let streaming = line.contains("\"streaming\"");
                println!("{line}");
                seen.push(line);
                if streaming {
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_secs(3));
        running.stop();
        assert!(seen.iter().any(|l| l.contains("\"streaming\"")), "never reached streaming: {seen:?}");
    }
}
