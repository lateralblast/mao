//! The saved selection, what the streamer and the outputs report, and keeping the Mac's
//! audio routing in line with the selection.
//!
//! - AirPlay receivers: the capture tap feeds one streamer for all of them.
//! - One output on this Mac: it becomes the Mac's system output, and macOS plays through it.
//!   The capture tap then passes the Mac's output through, instead of muting it.
//! - Nothing selected, or only AirPlay: the capture tap mutes the Mac's output, so nothing
//!   plays on this Mac.

use crate::paths;
use crate::stream::{self, Feed, Receiver, RunningStreamer, Tap};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Emitter};

pub const STATUS_EVENT: &str = "status-changed";

/// What is written to disk between launches.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct Saved {
    selected: Vec<Receiver>,
    /// The system output before mao switched it to a local output. Restored when that
    /// output is unticked, and when mao quits.
    #[serde(default)]
    original_output: Option<String>,
    /// Whether to show a notification when a receiver is lost. Off by default.
    #[serde(default)]
    notify_on_drop: bool,
    /// Whether AirPlay receivers are remembered between launches. Off by default.
    #[serde(default)]
    remember_receivers: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Link {
    pub state: String,
    pub message: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReceiverStatus {
    pub identifier: String,
    pub name: String,
    pub selected: bool,
    pub available: bool,
    /// "airplay" for a network receiver, "local" for an output on this Mac.
    pub kind: String,
    /// idle, connecting, streaming, ended, error or unavailable.
    pub state: String,
    pub message: String,
    /// The volume of a streaming AirPlay receiver, 0 to 100, once it has been reported.
    pub volume: Option<f64>,
    /// Whether the receiver is muted by mao.
    pub muted: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub receivers: Vec<ReceiverStatus>,
    pub active: bool,
    pub scanning: bool,
    pub error: Option<String>,
    /// Whether "mute all" is in effect.
    pub all_muted: bool,
}

#[derive(Default)]
struct Inner {
    saved: Saved,
    available: Vec<Receiver>,
    scanning: bool,
    /// A local refresh is in progress, and another was asked for while it ran.
    local_busy: bool,
    local_dirty: bool,
    links: BTreeMap<String, Link>,
    /// The volume each streaming AirPlay receiver reports, 0 to 100.
    volumes: BTreeMap<String, f64>,
    /// Receivers that have streamed since they were last lost (so a loss is announced once).
    streamed: BTreeSet<String>,
    /// Receivers muted by mao, with the volume to restore when they are unmuted.
    muted: BTreeMap<String, f64>,
    /// Whether "mute all" is in effect, and the receivers and outputs it muted (so that
    /// unmuting all restores only those, and leaves any muted by hand alone).
    all_muted: bool,
    mute_all_set: Vec<String>,
    /// The volume (0 to 100) and mute state of each selected output on this Mac.
    local_levels: BTreeMap<String, (f64, bool)>,
    /// Where the captured audio goes: the streamer's stdin. Kept here, so the tap can be restarted
    /// without disturbing the streamer.
    feed: Feed,
    /// The capture tap. It runs while AirPlay or nothing is selected.
    tap: Option<Tap>,
    /// Whether the running tap passes the Mac's output through (true) or mutes it (false).
    tap_pass: bool,
    /// Bumped whenever the tap starts or stops, so a late exit report is ignored.
    tap_generation: u64,
    /// The streamer for the selected AirPlay receivers. None when none is selected.
    streamer: Option<RunningStreamer>,
    /// Bumped on every streamer restart, so late output from an old streamer is ignored.
    generation: u64,
    /// At launch, with receivers remembered: they are not started until the search has found them.
    startup_pending: bool,
    error: Option<String>,
}

pub struct Controller {
    path: PathBuf,
    inner: Mutex<Inner>,
}

impl Controller {
    pub fn load(path: PathBuf) -> Self {
        let saved: Saved = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let saved = persistable(&saved);
        Controller {
            path,
            inner: Mutex::new(Inner {
                saved,
                ..Inner::default()
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn save(&self, saved: &Saved) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(&persistable(saved)).map_err(|e| e.to_string())?;
        fs::write(&self.path, text).map_err(|e| e.to_string())
    }

    pub fn status(&self) -> Status {
        let inner = self.lock();
        build_status(&inner)
    }

    /// Starts the saved selection, at launch. An output on this Mac that is the Mac's current
    /// system output is ticked, in place of any other output on this Mac, so the selection matches
    /// what the Mac is playing through.
    pub fn start_saved(self: &Arc<Self>, app: &AppHandle) {
        let mut inner = self.lock();
        inner.startup_pending = inner.saved.remember_receivers
            && inner.saved.selected.iter().any(|r| !stream::is_local(&r.identifier));
        if let Some(tap) = paths::tap_binary() {
            if let (Ok(current), Ok(local)) = (stream::system_output(&tap), stream::local_devices(&tap)) {
                if let Some(device) = local
                    .iter()
                    .find(|d| stream::local_uid(&d.identifier) == Some(current.as_str()))
                    .cloned()
                {
                    inner.saved.selected.retain(|r| !stream::is_local(&r.identifier));
                    inner.saved.selected.push(device.clone());
                    self.set_local_locked(app, &mut inner, local);
                    let saved = inner.saved.clone();
                    let _ = self.save(&saved);
                }
            }
        }
        self.sync_locked(app, &mut inner);
        drop(inner);
        let _ = app.emit(STATUS_EVENT, self.status());
    }

    /// Replaces the selection with `identifiers` and brings the Mac's routing in line with it.
    /// At most one output on this Mac is kept: the last one given.
    pub fn set_selected(self: &Arc<Self>, app: &AppHandle, identifiers: Vec<String>) -> Result<Status, String> {
        let mut inner = self.lock();
        let mut wanted = identifiers;
        if let Some(last_local) = wanted.iter().rev().find(|id| stream::is_local(id)).cloned() {
            wanted.retain(|id| !stream::is_local(id) || *id == last_local);
        }
        let merged = merge_selection(&inner.saved.selected, &inner.available, &wanted);
        inner.saved.selected = merged;
        self.save(&inner.saved)?;
        self.sync_locked(app, &mut inner);
        let status = build_status(&inner);
        drop(inner);
        let _ = app.emit(STATUS_EVENT, status.clone());
        Ok(status)
    }

    pub fn remember_receivers(&self) -> bool {
        self.lock().saved.remember_receivers
    }

    pub fn set_remember_receivers(&self, enabled: bool) -> Result<(), String> {
        let mut inner = self.lock();
        inner.saved.remember_receivers = enabled;
        let saved = inner.saved.clone();
        self.save(&saved)
    }

    pub fn notify_on_drop(&self) -> bool {
        self.lock().saved.notify_on_drop
    }

    pub fn set_notify_on_drop(&self, enabled: bool) -> Result<(), String> {
        let mut inner = self.lock();
        inner.saved.notify_on_drop = enabled;
        let saved = inner.saved.clone();
        self.save(&saved)
    }

    /// Sets the volume of a streaming AirPlay receiver, 0 to 100.
    pub fn set_volume(self: &Arc<Self>, app: &AppHandle, identifier: &str, level: f64) -> Result<(), String> {
        let inner = self.lock();
        let streamer = inner.streamer.as_ref().ok_or("that receiver is not streaming")?;
        streamer.set_volume(identifier, level.clamp(0.0, 100.0))?;
        drop(inner);
        let _ = app.emit(STATUS_EVENT, self.status());
        Ok(())
    }

    /// Mutes or unmutes a streaming AirPlay receiver. Muting keeps its volume to restore later.
    pub fn set_mute(self: &Arc<Self>, app: &AppHandle, identifier: &str, muted: bool) -> Result<(), String> {
        let mut guard = self.lock();
        let inner: &mut Inner = &mut guard;
        let streamer = inner.streamer.as_ref().ok_or("that receiver is not streaming")?;
        if muted {
            let restore = inner.volumes.get(identifier).copied().filter(|v| *v > 0.0).unwrap_or(50.0);
            inner.muted.entry(identifier.to_string()).or_insert(restore);
            streamer.set_volume(identifier, 0.0)?;
        } else {
            let level = inner.muted.remove(identifier).unwrap_or(50.0);
            streamer.set_volume(identifier, level)?;
        }
        let status = build_status(inner);
        drop(guard);
        let _ = app.emit(STATUS_EVENT, status);
        Ok(())
    }

    /// Sets the volume of a selected output on this Mac, 0 to 100.
    pub fn set_local_volume(self: &Arc<Self>, app: &AppHandle, identifier: &str, level: f64) -> Result<(), String> {
        let uid = stream::local_uid(identifier).ok_or("not an output on this Mac")?.to_string();
        let tap = paths::tap_binary().ok_or("the audio tap is missing from the app")?;
        let level = level.clamp(0.0, 100.0);
        stream::set_local_volume(&tap, &uid, level)?;
        let status = {
            let mut inner = self.lock();
            let muted = inner.local_levels.get(identifier).is_some_and(|(_, m)| *m);
            inner.local_levels.insert(identifier.to_string(), (level, muted));
            build_status(&inner)
        };
        let _ = app.emit(STATUS_EVENT, status);
        Ok(())
    }

    /// Mutes or unmutes a selected output on this Mac.
    pub fn set_local_mute(self: &Arc<Self>, app: &AppHandle, identifier: &str, muted: bool) -> Result<(), String> {
        let uid = stream::local_uid(identifier).ok_or("not an output on this Mac")?.to_string();
        let tap = paths::tap_binary().ok_or("the audio tap is missing from the app")?;
        stream::set_local_mute(&tap, &uid, muted)?;
        let status = {
            let mut inner = self.lock();
            let level = inner.local_levels.get(identifier).map(|(l, _)| *l).unwrap_or(50.0);
            inner.local_levels.insert(identifier.to_string(), (level, muted));
            build_status(&inner)
        };
        let _ = app.emit(STATUS_EVENT, status);
        Ok(())
    }

    /// Mutes everything that is streaming, or unmutes what "mute all" muted. Returns the new state.
    pub fn toggle_mute_all(self: &Arc<Self>, app: &AppHandle) -> Result<bool, String> {
        let muted = !self.lock().all_muted;
        self.set_mute_all(app, muted)?;
        Ok(muted)
    }

    pub fn set_mute_all(self: &Arc<Self>, app: &AppHandle, muted: bool) -> Result<(), String> {
        // Work out what to change, then make the changes without holding the lock.
        let (airplay, local, restore) = {
            let mut inner = self.lock();
            let streaming: Vec<String> = inner
                .saved
                .selected
                .iter()
                .filter(|r| inner.links.get(&r.identifier).is_some_and(|l| l.state == "streaming"))
                .map(|r| r.identifier.clone())
                .collect();
            let (local, airplay): (Vec<String>, Vec<String>) = streaming.into_iter().partition(|id| stream::is_local(id));
            let restore = std::mem::take(&mut inner.mute_all_set);
            let airplay_to_mute: Vec<String> = airplay.iter().filter(|id| !inner.muted.contains_key(*id)).cloned().collect();
            let local_to_mute: Vec<String> = local
                .iter()
                .filter(|id| !inner.local_levels.get(*id).is_some_and(|(_, m)| *m))
                .cloned()
                .collect();
            if muted {
                (airplay_to_mute, local_to_mute, Vec::new())
            } else {
                (Vec::new(), Vec::new(), restore)
            }
        };

        let mut first_error = None;
        let mut done = Vec::new();
        for identifier in airplay {
            match self.set_mute(app, &identifier, true) {
                Ok(()) => done.push(identifier),
                Err(error) => first_error = first_error.or(Some(error)),
            }
        }
        for identifier in local {
            match self.set_local_mute(app, &identifier, true) {
                Ok(()) => done.push(identifier),
                Err(error) => first_error = first_error.or(Some(error)),
            }
        }
        for identifier in restore {
            let result = if stream::is_local(&identifier) {
                self.set_local_mute(app, &identifier, false)
            } else {
                self.set_mute(app, &identifier, false)
            };
            if let Err(error) = result {
                first_error = first_error.or(Some(error));
            }
        }

        {
            let mut inner = self.lock();
            inner.all_muted = muted;
            inner.mute_all_set = if muted { done } else { Vec::new() };
        }
        let _ = app.emit(STATUS_EVENT, self.status());
        first_error.map_or(Ok(()), Err)
    }

    /// Looks for receivers on the network in the background.
    pub fn refresh_scan(self: &Arc<Self>, app: &AppHandle) {
        {
            let mut inner = self.lock();
            if inner.scanning {
                return;
            }
            inner.scanning = true;
        }
        let _ = app.emit(STATUS_EVENT, self.status());

        let controller = Arc::clone(self);
        let app = app.clone();
        std::thread::spawn(move || {
            // None when the outputs on this Mac could not be listed, so a failed listing removes nothing.
            let local: Option<Vec<Receiver>> = paths::tap_binary().and_then(|tap| stream::local_devices(&tap).ok());
            let result = match paths::streamer(&app) {
                Some(runner) => stream::scan(&runner).map(|mut found| {
                    found.extend(local.clone().unwrap_or_default());
                    found
                }),
                None => Err("the streamer is missing from the app".to_string()),
            };
            {
                let mut inner = controller.lock();
                inner.scanning = false;
                match result {
                    Ok(found) => {
                        inner.available = dedupe(found);
                        // Keep the saved addresses of selected receivers current.
                        let available = inner.available.clone();
                        for saved in inner.saved.selected.iter_mut() {
                            if let Some(fresh) = available.iter().find(|r| r.identifier == saved.identifier) {
                                saved.address = fresh.address.clone();
                                saved.name = fresh.name.clone();
                            }
                        }
                        let mut changed = remap_local(&mut inner.saved.selected, &available);
                        if inner.startup_pending {
                            // The search has finished: keep only the remembered receivers it found.
                            inner.startup_pending = false;
                            inner
                                .saved
                                .selected
                                .retain(|r| stream::is_local(&r.identifier) || available.iter().any(|a| a.identifier == r.identifier));
                            changed = true;
                        }
                        if let Some(local) = &local {
                            changed |= prune_missing_locals(&mut inner.saved.selected, local);
                        }
                        if changed {
                            controller.sync_locked(&app, &mut inner);
                        }
                        let saved = inner.saved.clone();
                        let _ = controller.save(&saved);
                    }
                    Err(error) => {
                        // The search failed: start the remembered receivers anyway, and let errors show.
                        if inner.startup_pending {
                            inner.startup_pending = false;
                            controller.sync_locked(&app, &mut inner);
                        }
                        inner.error = Some(error);
                    }
                }
            }
            let _ = app.emit(STATUS_EVENT, controller.status());
        });
    }

    /// Re-reads this Mac's outputs, for when a device is plugged in or removed.
    /// Quick, so it does not touch the AirPlay list.
    pub fn refresh_local(self: &Arc<Self>, app: &AppHandle) {
        {
            let mut inner = self.lock();
            if inner.local_busy {
                inner.local_dirty = true;
                return;
            }
            inner.local_busy = true;
        }
        loop {
            let result = match paths::tap_binary() {
                Some(tap) => stream::local_devices(&tap),
                None => Err("the audio tap is missing from the app".to_string()),
            };
            let mut inner = self.lock();
            if let Ok(local) = result {
                self.set_local_locked(app, &mut inner, local);
            }
            read_local_levels_locked(&mut inner);
            if inner.local_dirty {
                // Another change arrived while we were reading: read again.
                inner.local_dirty = false;
                continue;
            }
            inner.local_busy = false;
            let status = build_status(&inner);
            drop(inner);
            let _ = app.emit(STATUS_EVENT, status);
            return;
        }
    }

    /// Records the outputs on this Mac. A saved selection that moved to a new identifier is
    /// remapped, and the routing is brought in line with it.
    fn set_local_locked(self: &Arc<Self>, app: &AppHandle, inner: &mut Inner, local: Vec<Receiver>) {
        inner.available.retain(|r| !stream::is_local(&r.identifier));
        let mut changed = remap_local(&mut inner.saved.selected, &local);
        changed |= prune_missing_locals(&mut inner.saved.selected, &local);
        inner.available.extend(dedupe(local));
        if changed {
            let saved = inner.saved.clone();
            let _ = self.save(&saved);
            self.sync_locked(app, inner);
        }
    }

    /// Stops the tap, and restores the system output if mao changed it. Called when the app quits.
    pub fn shutdown(&self) {
        let mut inner = self.lock();
        inner.generation += 1;
        inner.tap_generation += 1;
        if let Some(streamer) = inner.streamer.take() {
            streamer.stop();
        }
        if let Some(tap) = inner.tap.take() {
            tap.stop();
        }
        restore_output_locked(self, &mut inner);
    }

    /// Starts the capture tap. `pass` lets the Mac's output play; otherwise it is muted.
    fn ensure_tap_locked(self: &Arc<Self>, app: &AppHandle, inner: &mut Inner, pass: bool) {
        if inner.tap.is_some() && inner.tap_pass == pass {
            return;
        }
        if let Some(tap) = inner.tap.take() {
            inner.tap_generation += 1;
            tap.stop();
        }
        let Some(tap_binary) = paths::tap_binary() else {
            inner.error = Some("the audio tap is missing from the app".into());
            return;
        };
        inner.tap_generation += 1;
        let generation = inner.tap_generation;
        let controller = Arc::clone(self);
        let exit_app = app.clone();
        match stream::start_tap(&tap_binary, pass, Arc::clone(&inner.feed), move || {
            controller.on_tap_exit(&exit_app, generation)
        }) {
            Ok(tap) => {
                inner.tap = Some(tap);
                inner.tap_pass = pass;
            }
            Err(error) => inner.error = Some(error),
        }
    }

    /// Brings the capture tap, the streamer and the Mac's output in line with the selection.
    /// A running streamer is changed in place: receivers are added or removed over its control
    /// socket, so the others keep streaming.
    fn sync_locked(self: &Arc<Self>, app: &AppHandle, inner: &mut Inner) {
        inner.error = None;

        let selected: Vec<String> = inner.saved.selected.iter().map(|r| r.identifier.clone()).collect();
        inner.links.retain(|identifier, _| selected.contains(identifier));

        // Remembered receivers wait for the launch-time search before they are started.
        let airplay: Vec<Receiver> = if inner.startup_pending {
            Vec::new()
        } else {
            inner
                .saved
                .selected
                .iter()
                .filter(|r| !stream::is_local(&r.identifier))
                .cloned()
                .collect()
        };
        let local: Option<String> = selected.iter().find(|id| stream::is_local(id)).cloned();

        // The capture tap feeds AirPlay, and mutes the Mac's output when nothing is selected.
        // With a local output, the Mac's output plays, so the tap passes it through.
        let needs_capture = !airplay.is_empty() || selected.is_empty();
        if needs_capture {
            self.ensure_tap_locked(app, inner, local.is_some());
        } else if let Some(tap) = inner.tap.take() {
            inner.tap_generation += 1;
            tap.stop();
        }

        if airplay.is_empty() {
            if let Some(streamer) = inner.streamer.take() {
                streamer.stop();
            }
        } else if inner.streamer.is_some() {
            // Change the running streamer: remove the receivers no longer selected, add the new ones.
            let current: Vec<String> = inner
                .streamer
                .as_ref()
                .map(|s| s.receivers.iter().map(|r| r.identifier.clone()).collect())
                .unwrap_or_default();
            let removed: Vec<String> = current
                .iter()
                .filter(|id| !airplay.iter().any(|r| &r.identifier == *id))
                .cloned()
                .collect();
            let added: Vec<Receiver> = airplay
                .iter()
                .filter(|r| !current.contains(&r.identifier))
                .cloned()
                .collect();
            let mut failure = None;
            if let Some(streamer) = inner.streamer.as_mut() {
                for identifier in &removed {
                    if let Err(error) = streamer.remove(identifier) {
                        failure = Some(error);
                    }
                    inner.links.remove(identifier);
                }
                for receiver in &added {
                    if let Err(error) = streamer.add(receiver) {
                        failure = Some(error);
                    }
                    inner.links.insert(
                        receiver.identifier.clone(),
                        Link {
                            state: "connecting".into(),
                            message: String::new(),
                        },
                    );
                }
            }
            for identifier in &removed {
                inner.volumes.remove(identifier);
                inner.muted.remove(identifier);
            }
            inner.error = failure;
        } else {
            // Nothing is streaming yet: start the streamer for all the AirPlay receivers.
            match (inner.tap.as_ref(), paths::streamer(app)) {
                (Some(_), Some(runner)) => {
                    inner.generation += 1;
                    let generation = inner.generation;
                    let controller = Arc::clone(self);
                    let event_app = app.clone();
                    let on_line = move |line: String| controller.on_line(&event_app, generation, &line);
                    for receiver in &airplay {
                        inner.links.insert(
                            receiver.identifier.clone(),
                            Link {
                                state: "connecting".into(),
                                message: String::new(),
                            },
                        );
                    }
                    inner.volumes.clear();
                    match stream::start_streamer(&inner.feed, &runner, &airplay, on_line) {
                        Ok(streamer) => inner.streamer = Some(streamer),
                        Err(error) => inner.error = Some(error),
                    }
                }
                _ => inner.error = Some("the audio tap or streamer is missing from the app".into()),
            }
        }

        match local {
            Some(identifier) => {
                let uid = stream::local_uid(&identifier).unwrap_or_default().to_string();
                let current = paths::tap_binary().and_then(|tap| stream::system_output(&tap).ok());
                // Remember the output the Mac had before mao changed it.
                if inner.saved.original_output.is_none() {
                    if let Some(current) = current.filter(|current| *current != uid) {
                        inner.saved.original_output = Some(current);
                        let saved = inner.saved.clone();
                        let _ = self.save(&saved);
                    }
                }
                let result = paths::tap_binary()
                    .ok_or_else(|| "the audio tap is missing from the app".to_string())
                    .and_then(|tap| stream::set_system_output(&tap, &uid));
                match result {
                    Ok(()) => {
                        read_local_levels_locked(inner);
                        inner.links.insert(
                            identifier.clone(),
                            Link {
                                state: "streaming".into(),
                                message: String::new(),
                            },
                        );
                    }
                    Err(error) => {
                        inner.error = Some(error.clone());
                        inner.links.insert(
                            identifier.clone(),
                            Link {
                                state: "error".into(),
                                message: error,
                            },
                        );
                    }
                }
            }
            None => restore_output_locked(self, inner),
        }
    }

    fn on_line(&self, app: &AppHandle, generation: u64, line: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        if value["event"] == "volume" {
            let (Some(identifier), Some(level)) = (value["identifier"].as_str(), value["level"].as_f64()) else {
                return;
            };
            let status = {
                let mut inner = self.lock();
                if inner.generation != generation {
                    return;
                }
                inner.volumes.insert(identifier.to_string(), level);
                // A muted receiver may report its own volume again (after a reconnect): mute it again.
                if level > 0.0 && inner.muted.contains_key(identifier) {
                    if let Some(streamer) = inner.streamer.as_ref() {
                        let _ = streamer.set_volume(identifier, 0.0);
                    }
                }
                build_status(&inner)
            };
            let _ = app.emit(STATUS_EVENT, status);
            return;
        }
        if value["event"] != "receiver" {
            return;
        }
        let (Some(identifier), Some(state)) = (value["identifier"].as_str(), value["state"].as_str()) else {
            return;
        };
        let message = value["message"].as_str().unwrap_or_default().to_string();
        let mut lost_name = None;
        let status = {
            let mut inner = self.lock();
            if inner.generation != generation {
                return;
            }
            // A receiver that was streaming and is now lost is announced once, as a notification.
            if state == "streaming" {
                inner.streamed.insert(identifier.to_string());
            } else if state == "error"
                && message.starts_with("lost")
                && inner.streamed.remove(identifier)
                && inner.saved.notify_on_drop
            {
                lost_name = inner.saved.selected.iter().find(|r| r.identifier == identifier).map(|r| r.name.clone());
            }
            inner.links.insert(
                identifier.to_string(),
                Link {
                    state: state.to_string(),
                    message,
                },
            );
            build_status(&inner)
        };
        if let Some(name) = lost_name {
            notify_lost(app, &name);
        }
        let _ = app.emit(STATUS_EVENT, status);
    }

    fn on_tap_exit(&self, app: &AppHandle, generation: u64) {
        let status = {
            let mut inner = self.lock();
            if inner.tap_generation != generation {
                return;
            }
            inner.error = Some("system audio capture stopped; quit and reopen mao to resume".into());
            if let Some(streamer) = inner.streamer.take() {
                streamer.stop();
            }
            if let Some(tap) = inner.tap.take() {
                tap.stop();
            }
            build_status(&inner)
        };
        let _ = app.emit(STATUS_EVENT, status);
    }
}

/// Puts back the system output that mao changed, if it changed one. Forgets it either way.
fn restore_output_locked(controller: &Controller, inner: &mut Inner) {
    let Some(original) = inner.saved.original_output.take() else {
        return;
    };
    let saved = inner.saved.clone();
    let _ = controller.save(&saved);
    let restored = paths::tap_binary()
        .ok_or_else(|| "the audio tap is missing from the app".to_string())
        .and_then(|tap| stream::set_system_output(&tap, &original));
    if let Err(error) = restored {
        inner.error = Some(error);
    }
}

/// The volume to show for a receiver: the output's own level, or the AirPlay receiver's reported one.
fn volume_of(inner: &Inner, identifier: &str) -> Option<f64> {
    if stream::is_local(identifier) {
        inner.local_levels.get(identifier).map(|(level, _)| *level)
    } else {
        inner.volumes.get(identifier).copied()
    }
}

fn muted_of(inner: &Inner, identifier: &str) -> bool {
    if stream::is_local(identifier) {
        inner.local_levels.get(identifier).is_some_and(|(_, muted)| *muted)
    } else {
        inner.muted.contains_key(identifier)
    }
}

/// Reads the volume and mute state of the selected outputs on this Mac.
fn read_local_levels_locked(inner: &mut Inner) {
    let Some(tap) = paths::tap_binary() else {
        return;
    };
    let selected: Vec<String> = inner
        .saved
        .selected
        .iter()
        .filter(|r| stream::is_local(&r.identifier))
        .map(|r| r.identifier.clone())
        .collect();
    inner.local_levels.retain(|identifier, _| selected.contains(identifier));
    for identifier in selected {
        if let Some(uid) = stream::local_uid(&identifier) {
            if let Ok(level) = stream::local_volume(&tap, uid) {
                inner.local_levels.insert(identifier, level);
            }
        }
    }
}

/// A macOS notification that a ticked receiver has dropped off the network.
fn notify_lost(app: &AppHandle, name: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app
        .notification()
        .builder()
        .title("mao")
        .body(format!("{name} dropped off the network. Trying again every 10 seconds."))
        .show();
}

fn kind_of(identifier: &str) -> String {
    if stream::is_local(identifier) { "local" } else { "airplay" }.to_string()
}

fn build_status(inner: &Inner) -> Status {
    let selected_ids: Vec<&str> = inner.saved.selected.iter().map(|r| r.identifier.as_str()).collect();
    let mut receivers: Vec<ReceiverStatus> = inner
        .available
        .iter()
        .map(|r| {
            let selected = selected_ids.contains(&r.identifier.as_str());
            let link = inner.links.get(&r.identifier).cloned();
            let (state, message) = match (selected, link) {
                (true, Some(link)) => (link.state, link.message),
                (true, None) => ("connecting".to_string(), String::new()),
                (false, _) => ("idle".to_string(), String::new()),
            };
            ReceiverStatus {
                identifier: r.identifier.clone(),
                name: r.name.clone(),
                selected,
                available: true,
                kind: kind_of(&r.identifier),
                state,
                message,
                volume: volume_of(inner, &r.identifier),
                muted: muted_of(inner, &r.identifier),
            }
        })
        .collect();

    // Selected receivers that discovery did not find are kept, so they can be unticked.
    for saved in &inner.saved.selected {
        if receivers.iter().any(|r| r.identifier == saved.identifier) {
            continue;
        }
        receivers.push(ReceiverStatus {
            identifier: saved.identifier.clone(),
            name: saved.name.clone(),
            selected: true,
            available: false,
            kind: kind_of(&saved.identifier),
            state: "unavailable".into(),
            message: "not found on the network".into(),
            volume: volume_of(inner, &saved.identifier),
            muted: muted_of(inner, &saved.identifier),
        });
    }
    receivers.sort_by_key(|r| r.name.to_lowercase());

    Status {
        receivers,
        all_muted: inner.all_muted,
        active: inner.streamer.is_some() || inner.saved.selected.iter().any(|r| stream::is_local(&r.identifier)),
        scanning: inner.scanning,
        error: inner.error.clone(),
    }
}

/// The new selection: each wanted identifier, using the freshest details we have.
pub fn merge_selection(saved: &[Receiver], available: &[Receiver], wanted: &[String]) -> Vec<Receiver> {
    let mut merged: Vec<Receiver> = Vec::new();
    for identifier in wanted {
        if merged.iter().any(|r| &r.identifier == identifier) {
            continue;
        }
        let found = available
            .iter()
            .find(|r| &r.identifier == identifier)
            .or_else(|| saved.iter().find(|r| &r.identifier == identifier));
        if let Some(receiver) = found {
            merged.push(receiver.clone());
        }
    }
    merged
}

/// A saved local selection can carry an identifier from an older version, or a device
/// that now has another identifier. This points each such selection at the current output
/// with the same name. Returns whether anything changed.
pub fn remap_local(saved: &mut [Receiver], available: &[Receiver]) -> bool {
    let mut changed = false;
    for receiver in saved.iter_mut() {
        if !stream::is_local(&receiver.identifier) || available.iter().any(|a| a.identifier == receiver.identifier) {
            continue;
        }
        if let Some(found) = available
            .iter()
            .find(|a| stream::is_local(&a.identifier) && a.name == receiver.name)
        {
            receiver.identifier = found.identifier.clone();
            changed = true;
        }
    }
    changed
}

/// What is kept between launches: the outputs on this Mac only. AirPlay receivers are not
/// remembered, because they are only known once discovery has finished, and ticking one before
/// then fails. At launch the Mac's current output decides the local selection.
fn persistable(saved: &Saved) -> Saved {
    let mut kept = saved.clone();
    if !saved.remember_receivers {
        kept.selected.retain(|r| stream::is_local(&r.identifier));
    }
    kept
}

/// Removes selected outputs on this Mac that are no longer present. Returns whether any were removed.
pub fn prune_missing_locals(saved: &mut Vec<Receiver>, present: &[Receiver]) -> bool {
    let before = saved.len();
    saved.retain(|r| !stream::is_local(&r.identifier) || present.iter().any(|p| p.identifier == r.identifier));
    saved.len() != before
}

/// One entry per receiver. Discovery can report the same device at several addresses.
fn dedupe(found: Vec<Receiver>) -> Vec<Receiver> {
    let mut unique: Vec<Receiver> = Vec::new();
    for receiver in found {
        if !unique.iter().any(|r| r.identifier == receiver.identifier) {
            unique.push(receiver);
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receiver(id: &str, name: &str, address: &str) -> Receiver {
        Receiver {
            identifier: id.into(),
            name: name.into(),
            address: address.into(),
        }
    }

    #[test]
    fn merge_uses_fresh_details_for_wanted_receivers() {
        let saved = vec![receiver("A", "Kitchen", "10.0.0.1")];
        let available = vec![receiver("A", "Kitchen", "10.0.0.9"), receiver("B", "Lounge", "10.0.0.2")];
        let merged = merge_selection(&saved, &available, &["A".into(), "B".into()]);
        assert_eq!(merged[0].address, "10.0.0.9");
        assert_eq!(merged[1].name, "Lounge");
    }

    #[test]
    fn merge_keeps_saved_receivers_that_are_offline() {
        let saved = vec![receiver("A", "Kitchen", "10.0.0.1")];
        let merged = merge_selection(&saved, &[], &["A".into()]);
        assert_eq!(merged, saved);
    }

    #[test]
    fn merge_drops_unknown_and_repeated_identifiers() {
        let available = vec![receiver("A", "Kitchen", "10.0.0.1")];
        let merged = merge_selection(&[], &available, &["A".into(), "A".into(), "Z".into()]);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn status_marks_selected_and_offline_receivers() {
        let inner = Inner {
            saved: Saved {
                selected: vec![receiver("A", "Kitchen", "10.0.0.1"), receiver("C", "Study", "10.0.0.3")],
                original_output: None,
                notify_on_drop: false,
                remember_receivers: false,
            },
            available: vec![receiver("A", "Kitchen", "10.0.0.1"), receiver("B", "Lounge", "10.0.0.2")],
            links: BTreeMap::from([(
                "A".to_string(),
                Link {
                    state: "streaming".into(),
                    message: String::new(),
                },
            )]),
            ..Inner::default()
        };
        let status = build_status(&inner);
        let kitchen = status.receivers.iter().find(|r| r.identifier == "A").unwrap();
        assert!(kitchen.selected && kitchen.available);
        assert_eq!(kitchen.state, "streaming");
        let lounge = status.receivers.iter().find(|r| r.identifier == "B").unwrap();
        assert!(!lounge.selected);
        assert_eq!(lounge.state, "idle");
        let study = status.receivers.iter().find(|r| r.identifier == "C").unwrap();
        assert!(study.selected && !study.available);
        assert_eq!(study.state, "unavailable");
        assert!(!status.active);
    }

    #[test]
    fn remap_moves_a_saved_local_output_to_its_current_identifier() {
        let mut saved = vec![receiver("local:External Headphones", "External Headphones", "")];
        let available = vec![receiver("local:BuiltInHeadphoneOutputDevice", "External Headphones", "")];
        assert!(remap_local(&mut saved, &available));
        assert_eq!(saved[0].identifier, "local:BuiltInHeadphoneOutputDevice");
        assert!(!remap_local(&mut saved, &available), "a second pass changes nothing");
    }

    #[test]
    fn remap_leaves_airplay_and_missing_outputs_alone() {
        let mut saved = vec![receiver("B2:5B", "Kitchen", "10.0.0.1"), receiver("local:Gone", "Gone", "")];
        let available = vec![receiver("local:Other", "Other", "")];
        assert!(!remap_local(&mut saved, &available));
        assert_eq!(saved[1].identifier, "local:Gone");
    }

    #[test]
    fn only_outputs_on_this_mac_are_kept_between_launches() {
        let saved = Saved {
            selected: vec![
                receiver("B2:5B", "Kitchen", "10.0.0.1"),
                receiver("local:BuiltInHeadphoneOutputDevice", "External Headphones", ""),
            ],
            original_output: Some("BuiltInSpeakerDevice".into()),
            notify_on_drop: false,
            remember_receivers: false,
        };
        let kept = persistable(&saved);
        assert_eq!(kept.selected.len(), 1);
        assert_eq!(kept.selected[0].name, "External Headphones");
        assert_eq!(kept.original_output.as_deref(), Some("BuiltInSpeakerDevice"));
    }

    #[test]
    fn prune_removes_only_missing_outputs_on_this_mac() {
        let mut saved = vec![
            receiver("local:Gone", "Gone", ""),
            receiver("local:Here", "Here", ""),
            receiver("B2:5B", "Kitchen", "10.0.0.1"),
        ];
        let present = vec![receiver("local:Here", "Here", "")];
        assert!(prune_missing_locals(&mut saved, &present));
        let ids: Vec<&str> = saved.iter().map(|r| r.identifier.as_str()).collect();
        assert_eq!(ids, vec!["local:Here", "B2:5B"]);
        assert!(!prune_missing_locals(&mut saved, &present), "a second pass changes nothing");
    }

    #[test]
    fn airplay_receivers_are_kept_only_when_remembered() {
        let mut saved = Saved {
            selected: vec![receiver("B2:5B", "Kitchen", "10.0.0.1")],
            ..Saved::default()
        };
        assert!(persistable(&saved).selected.is_empty());
        saved.remember_receivers = true;
        assert_eq!(persistable(&saved).selected.len(), 1);
    }

    #[test]
    fn kind_follows_the_identifier_prefix() {
        assert_eq!(kind_of("local:External Headphones"), "local");
        assert_eq!(kind_of("B2:5B:17:C7:B2:04"), "airplay");
    }

    #[test]
    fn dedupe_keeps_the_first_address_per_identifier() {
        let found = vec![receiver("A", "Kitchen", "10.0.0.1"), receiver("A", "Kitchen", "10.0.0.2")];
        assert_eq!(dedupe(found).len(), 1);
    }
}
