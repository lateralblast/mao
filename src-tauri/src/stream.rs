//! The system-audio tap and the Python streamer, and the pipe that joins them.
//!
//! The tap runs for as long as mao does and mutes the Mac's own output while it
//! runs. Its audio goes to whichever streamer is attached, or nowhere when none is.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

/// A receiver, or a device on this Mac, as reported by discovery.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Receiver {
    pub identifier: String,
    pub name: String,
    pub address: String,
}

type Feed = Arc<Mutex<Option<ChildStdin>>>;

/// The running tap. Its audio goes to the attached streamer, or is discarded.
pub struct Tap {
    child: Child,
    // Keeping the tap's stdin open keeps it running. Closing it makes the tap exit.
    stdin: Option<ChildStdin>,
    feed: Feed,
}

impl Tap {
    /// Sends the tap's audio to `streamer_stdin` from now on.
    pub fn attach(&self, streamer_stdin: ChildStdin) {
        if let Ok(mut feed) = self.feed.lock() {
            *feed = Some(streamer_stdin);
        }
    }

    /// Stops sending audio anywhere. Closing the streamer's stdin lets it finish.
    pub fn detach(&self) {
        if let Ok(mut feed) = self.feed.lock() {
            *feed = None;
        }
    }

    pub fn stop(mut self) {
        self.detach();
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts the tap. `on_exit` runs if the tap stops before it is stopped.
pub fn start_tap(tap: &Path, on_exit: impl FnOnce() + Send + 'static) -> Result<Tap, String> {
    let mut child = Command::new(tap)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start the audio tap: {e}"))?;
    let stdin = child.stdin.take();
    let mut out = child.stdout.take().ok_or("no tap stdout")?;
    let err = child.stderr.take().ok_or("no tap stderr")?;
    let feed: Feed = Arc::new(Mutex::new(None));

    let relay_feed = Arc::clone(&feed);
    thread::spawn(move || {
        let mut buffer = [0u8; 16384];
        loop {
            match out.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut feed) = relay_feed.lock() {
                        if let Some(target) = feed.as_mut() {
                            if target.write_all(&buffer[..n]).is_err() {
                                *feed = None;
                            }
                        }
                    }
                }
            }
        }
        on_exit();
    });
    thread::spawn(move || log_lines("mao-tap", err));

    Ok(Tap { child, stdin, feed })
}

/// A running Python streamer for a set of targets.
pub struct Streamer {
    child: Child,
}

impl Streamer {
    pub fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts the streamer for `receivers` and attaches it to the tap.
/// `on_line` gets each JSON line the streamer prints.
pub fn start_streamer(
    tap: &Tap,
    python: &Path,
    script: &Path,
    receivers: &[Receiver],
    on_line: impl Fn(String) + Send + 'static,
) -> Result<Streamer, String> {
    let mut args: Vec<String> = vec!["stream".into()];
    args.extend(receivers.iter().map(target));
    let mut child = Command::new(python)
        .arg(script)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start the streamer ({}): {e}", python.display()))?;
    let stdin = child.stdin.take().ok_or("no streamer stdin")?;
    let stdout = child.stdout.take().ok_or("no streamer stdout")?;
    let stderr = child.stderr.take().ok_or("no streamer stderr")?;

    tap.attach(stdin);
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            on_line(line);
        }
    });
    thread::spawn(move || log_lines("mao-stream", stderr));
    Ok(Streamer { child })
}

/// The argument the streamer expects for a target: `local:NAME`, or `IDENTIFIER=ADDRESS`.
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

fn log_lines(source: &str, reader: impl Read) {
    for line in BufReader::new(reader).lines().map_while(Result::ok) {
        eprintln!("[{source}] {line}");
    }
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
        let tap_path = crate::paths::tap_binary().expect("the tap binary is built");
        let python = crate::paths::python();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("python").join("mao_stream.py");
        let receiver = Receiver {
            identifier: "B2:5B:17:C7:B2:04".into(),
            name: "Kitchen".into(),
            address: "192.168.1.62".into(),
        };

        let tap = start_tap(&tap_path, || {}).expect("start tap");
        let (tx, rx) = mpsc::channel();
        let streamer = start_streamer(&tap, &python, &script, &[receiver], move |line| {
            let _ = tx.send(line);
        })
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
        streamer.stop();
        tap.stop();
        assert!(seen.iter().any(|l| l.contains("\"streaming\"")), "never reached streaming: {seen:?}");
    }
}
