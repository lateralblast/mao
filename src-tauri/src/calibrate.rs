//! Sync calibration. Each AirPlay receiver is measured on its own: the others are muted while it
//! plays a click track, and the Mac's microphone records the room. How late each receiver's clicks
//! arrive tells how far it plays behind; the receivers are then held back so they all line up with
//! the one that plays latest. This needs the microphone, and the user is asked before it starts.

use crate::controller::Controller;
use crate::{paths, stream};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// The event the popover listens to for progress.
pub const EVENT: &str = "calibration";
/// How long to record. The click track is 40 seconds, and playback starts after the lead-in.
const RECORD_SECONDS: f64 = 44.0;
/// Time between starting the recording and starting the click track.
const LEAD_IN: Duration = Duration::from_secs(2);

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Measurement {
    pub name: String,
    pub lag_ms: f64,
    pub delay_ms: f64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// running, done, cancelled or error.
    pub state: String,
    pub step: String,
    pub results: Vec<Measurement>,
    pub message: String,
}

fn emit(app: &AppHandle, state: &str, step: &str, results: Vec<Measurement>, message: &str) {
    let _ = app.emit(
        EVENT,
        Progress {
            state: state.into(),
            step: step.into(),
            results,
            message: message.into(),
        },
    );
}

/// Starts a calibration in the background. Errors (not enough receivers, and so on) come back at once.
pub fn start(controller: Arc<Controller>, app: AppHandle) -> Result<(), String> {
    let targets = controller.calibration_targets()?;
    let cancel = controller.set_calibrating(true);
    thread::spawn(move || {
        let outcome = measure(&controller, &app, &targets, &cancel);
        controller.set_calibrating(false);
        match outcome {
            Ok(results) => emit(&app, "done", "Calibration finished", results, ""),
            Err(error) if cancel.load(Ordering::SeqCst) => emit(&app, "cancelled", "Cancelled", Vec::new(), &error),
            Err(error) => emit(&app, "error", "Calibration failed", Vec::new(), &error),
        }
    });
    Ok(())
}

/// The helpers and the files a calibration uses.
struct Workspace {
    tap: std::path::PathBuf,
    runner: paths::Streamer,
    folder: std::path::PathBuf,
    clicks: std::path::PathBuf,
}

fn measure(
    controller: &Arc<Controller>,
    app: &AppHandle,
    targets: &[crate::stream::Receiver],
    cancel: &Arc<AtomicBool>,
) -> Result<Vec<Measurement>, String> {
    let tap = paths::tap_binary().ok_or("the audio tap is missing from the app")?;
    let runner = paths::streamer(app).ok_or("the streamer is missing from the app")?;
    let folder = std::env::temp_dir().join("mao-calibration");
    std::fs::create_dir_all(&folder).map_err(|e| format!("could not make {}: {e}", folder.display()))?;
    let clicks = folder.join("clicks.wav");
    stream::write_click_track(&runner, &clicks)?;
    let work = Workspace {
        tap,
        runner,
        folder,
        clicks,
    };

    // Put back exactly what was there before: mutes, and the delays that were set.
    let was_muted: Vec<bool> = targets.iter().map(|t| controller.is_muted(&t.identifier)).collect();
    let saved_delays = controller.saved_delays();
    controller.apply_delays(BTreeMap::new())?;

    let result = measure_each(controller, app, targets, cancel, &work);

    for (target, muted) in targets.iter().zip(&was_muted) {
        if controller.is_muted(&target.identifier) != *muted {
            let _ = controller.set_mute(app, &target.identifier, *muted);
        }
    }
    match result {
        Ok(lags) => {
            let latest = lags.iter().map(|(_, lag)| *lag).fold(f64::MIN, f64::max);
            let mut delays = BTreeMap::new();
            let mut results = Vec::new();
            for (target, lag) in &lags {
                let delay = latest - lag;
                delays.insert(target.identifier.clone(), delay);
                results.push(Measurement {
                    name: target.name.clone(),
                    lag_ms: *lag,
                    delay_ms: delay,
                });
            }
            controller.apply_delays(delays)?;
            Ok(results)
        }
        Err(error) => {
            let _ = controller.apply_delays(saved_delays);
            Err(error)
        }
    }
}

fn measure_each(
    controller: &Arc<Controller>,
    app: &AppHandle,
    targets: &[crate::stream::Receiver],
    cancel: &Arc<AtomicBool>,
    work: &Workspace,
) -> Result<Vec<(crate::stream::Receiver, f64)>, String> {
    let mut lags = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return Err("calibration was cancelled".into());
        }
        let step = format!("Measuring {} ({} of {})", target.name, index + 1, targets.len());
        emit(app, "running", &step, Vec::new(), "");

        // Only this receiver should play: mute the others, and make sure this one is audible.
        for (other_index, other) in targets.iter().enumerate() {
            let want_muted = other_index != index;
            if controller.is_muted(&other.identifier) != want_muted {
                controller.set_mute(app, &other.identifier, want_muted)?;
            }
        }

        let recording = work.folder.join(format!("receiver-{index}.wav"));
        let mut recorder = stream::start_recording(&work.tap, RECORD_SECONDS, &recording)?;
        thread::sleep(LEAD_IN);
        let mut player = stream::start_playing(&work.clicks)?;
        // Wait for the recording to finish, stopping early if cancelled.
        loop {
            if recorder.try_wait().map_err(|e| e.to_string())?.is_some() {
                break;
            }
            if cancel.load(Ordering::SeqCst) {
                let _ = recorder.kill();
                let _ = player.kill();
                return Err("calibration was cancelled".into());
            }
            thread::sleep(Duration::from_millis(200));
        }
        let _ = player.kill();
        let _ = player.wait();

        let measured = stream::find_lag(&work.runner, &recording, &work.clicks);
        // The recording is only needed for the measurement, so it is deleted straight away.
        let _ = std::fs::remove_file(&recording);
        lags.push((target.clone(), measured?.lag_ms));
    }
    Ok(lags)
}

/// Stops a calibration that is running. It finishes its current step and puts everything back.
pub fn cancel(controller: &Controller) {
    controller.cancel_calibration();
}
