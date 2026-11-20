//! The system-audio tap, the AirPlay streamer, and the Mac's output switching.
//!
//! The audio goes through a `Feed`: the tap writes into it, and it is the streamer's stdin. The
//! feed belongs to the controller, so restarting the tap (to change how the Mac's output is
//! handled) does not disturb the streamer. The streamer takes receivers added and removed over a
//! Unix socket, so changing the selection does not restart it.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
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

/// Where the audio goes: the streamer's stdin, when a streamer is running.
pub type Feed = Arc<Mutex<Option<ChildStdin>>>;

/// The socket the streamer listens on for receivers added and removed.
pub fn control_socket() -> PathBuf {
    std::env::temp_dir().join(format!("mao-stream-{}.sock", std::process::id()))
}

/// The running tap. It writes its audio into the feed, and is stopped on its own.
pub struct Tap {
    child: Child,
    // Keeping the tap's stdin open keeps it running. Closing it makes the tap exit.
    stdin: Option<ChildStdin>,
}

impl Tap {
    pub fn stop(mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts the tap. With `pass`, the Mac's output plays; otherwise it is muted. Its audio goes to
/// `feed`. `on_exit` runs if the tap stops before it is stopped.
pub fn start_tap(tap: &Path, pass: bool, feed: Feed, on_exit: impl FnOnce() + Send + 'static) -> Result<Tap, String> {
    let mut command = Command::new(tap);
    if pass {
        command.arg("--pass");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start the audio tap: {e}"))?;
    let stdin = child.stdin.take();
    let mut out = child.stdout.take().ok_or("no tap stdout")?;
    let err = child.stderr.take().ok_or("no tap stderr")?;

    thread::spawn(move || {
        let mut buffer = [0u8; 16384];
        loop {
            match out.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut feed) = feed.lock() {
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

    Ok(Tap { child, stdin })
}

/// How to run the streamer: the frozen binary, or the Python script in the mao environment.
pub use crate::paths::Streamer as Runner;

/// A running streamer for the AirPlay receivers.
pub struct RunningStreamer {
    child: Child,
    feed: Feed,
    socket: PathBuf,
    /// The receivers it is streaming to, as the controller last set them.
    pub receivers: Vec<Receiver>,
}

impl RunningStreamer {
    /// Starts streaming to `receiver` as well.
    pub fn add(&mut self, receiver: &Receiver) -> Result<(), String> {
        self.send(&format!("add {} {}", receiver.identifier, receiver.address))?;
        self.receivers.push(receiver.clone());
        Ok(())
    }

    /// Stops streaming to the receiver `identifier`.
    pub fn remove(&mut self, identifier: &str) -> Result<(), String> {
        self.send(&format!("remove {identifier}"))?;
        self.receivers.retain(|r| r.identifier != identifier);
        Ok(())
    }

    fn send(&self, line: &str) -> Result<(), String> {
        let mut socket = UnixStream::connect(&self.socket).map_err(|e| format!("could not reach the streamer: {e}"))?;
        socket
            .write_all(format!("{line}\n").as_bytes())
            .map_err(|e| format!("could not reach the streamer: {e}"))
    }

    pub fn stop(mut self) {
        if let Ok(mut feed) = self.feed.lock() {
            *feed = None;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Starts the streamer for `receivers`, with its audio taken from `feed`. `on_line` gets each
/// JSON line the streamer prints.
pub fn start_streamer(
    feed: &Feed,
    runner: &Runner,
    receivers: &[Receiver],
    on_line: impl Fn(String) + Send + 'static,
) -> Result<RunningStreamer, String> {
    let socket = control_socket();
    let _ = std::fs::remove_file(&socket);
    let mut args: Vec<String> = vec!["stream".into(), "--control".into(), socket.to_string_lossy().into_owned()];
    args.extend(receivers.iter().map(|r| format!("{}={}", r.identifier, r.address)));
    let mut child = runner
        .command(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start the streamer: {e}"))?;
    let stdin = child.stdin.take().ok_or("no streamer stdin")?;
    let stdout = child.stdout.take().ok_or("no streamer stdout")?;
    let stderr = child.stderr.take().ok_or("no streamer stderr")?;

    if let Ok(mut feed) = feed.lock() {
        *feed = Some(stdin);
    }
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            on_line(line);
        }
    });
    thread::spawn(move || log_lines("mao-stream", stderr));
    Ok(RunningStreamer {
        child,
        feed: Arc::clone(feed),
        socket,
        receivers: receivers.to_vec(),
    })
}

/// Whether an identifier names an output device on this Mac rather than an AirPlay receiver.
pub fn is_local(identifier: &str) -> bool {
    identifier.starts_with("local:")
}

/// The uid of a local output identifier (`local:<uid>`).
pub fn local_uid(identifier: &str) -> Option<&str> {
    identifier.strip_prefix("local:")
}

/// The UID of the Mac's system output.
pub fn system_output(tap: &Path) -> Result<String, String> {
    let output = Command::new(tap)
        .arg("output")
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("could not read the system output: {e}"))?;
    if !output.status.success() {
        return Err("could not read the system output".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Makes the output device `uid` the Mac's system output.
pub fn set_system_output(tap: &Path, uid: &str) -> Result<(), String> {
    let output = Command::new(tap)
        .arg("set-output")
        .arg(uid)
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("could not set the system output: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("could not switch the Mac's output to {uid}"))
    }
}

fn log_lines(source: &str, reader: impl Read) {
    for line in BufReader::new(reader).lines().map_while(Result::ok) {
        eprintln!("[{source}] {line}");
    }
}

/// Lists the AirPlay receivers on the network. Blocks for a few seconds.
pub fn scan(runner: &Runner) -> Result<Vec<Receiver>, String> {
    run_json(runner, "scan")
}

/// Lists this Mac's output devices, as the tap helper reports them.
pub fn local_devices(tap: &Path) -> Result<Vec<Receiver>, String> {
    let output = Command::new(tap)
        .arg("devices")
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("could not list the outputs: {e}"))?;
    if !output.status.success() {
        return Err("could not list the outputs".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("could not read the outputs: {e}"))
}

fn run_json(runner: &Runner, command: &str) -> Result<Vec<Receiver>, String> {
    let output = runner
        .command(&[command.to_string()])
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("could not run the streamer: {e}"))?;
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
        let runner = Runner::Script {
            python: PathBuf::from(std::env::var("MAO_PYTHON").expect("set MAO_PYTHON to the mao environment's python")),
            script: Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("python").join("mao_stream.py"),
        };
        let receiver = Receiver {
            identifier: "B2:5B:17:C7:B2:04".into(),
            name: "Kitchen".into(),
            address: "192.168.1.62".into(),
        };

        let feed: Feed = Arc::new(Mutex::new(None));
        let tap = start_tap(&tap_path, false, Arc::clone(&feed), || {}).expect("start tap");
        let (tx, rx) = mpsc::channel();
        let streamer = start_streamer(&feed, &runner, &[receiver], move |line| {
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
