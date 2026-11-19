//! The saved receiver selection, what the streamer reports about each receiver,
//! and restarting the stream whenever the selection changes.

use crate::paths;
use crate::stream::{self, Receiver, RunningStreamer, Tap};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Link {
    pub state: String,
    pub message: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
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
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub receivers: Vec<ReceiverStatus>,
    pub active: bool,
    pub scanning: bool,
    pub error: Option<String>,
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
    /// The system-audio tap. It runs for the whole session and mutes the Mac's output.
    tap: Option<Tap>,
    /// Bumped whenever the tap starts or stops, so a late exit report is ignored.
    tap_generation: u64,
    /// The streamer for the current selection. None when nothing is selected.
    streamer: Option<RunningStreamer>,
    /// Bumped on every streamer restart, so late output from an old streamer is ignored.
    generation: u64,
    error: Option<String>,
}

pub struct Controller {
    path: PathBuf,
    inner: Mutex<Inner>,
}

impl Controller {
    pub fn load(path: PathBuf) -> Self {
        let saved = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
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
        let text = serde_json::to_string_pretty(saved).map_err(|e| e.to_string())?;
        fs::write(&self.path, text).map_err(|e| e.to_string())
    }

    pub fn status(&self) -> Status {
        let inner = self.lock();
        build_status(&inner)
    }

    /// Starts the tap and resumes the saved selection, at launch.
    pub fn start_saved(self: &Arc<Self>, app: &AppHandle) {
        let mut inner = self.lock();
        self.ensure_tap_locked(app, &mut inner);
        self.restart_streamer_locked(app, &mut inner);
        drop(inner);
        let _ = app.emit(STATUS_EVENT, self.status());
    }

    /// Replaces the selection with `identifiers` and restarts the streamer.
    pub fn set_selected(self: &Arc<Self>, app: &AppHandle, identifiers: Vec<String>) -> Result<Status, String> {
        let mut inner = self.lock();
        let merged = merge_selection(&inner.saved.selected, &inner.available, &identifiers);
        inner.saved.selected = merged;
        self.save(&inner.saved)?;
        self.ensure_tap_locked(app, &mut inner);
        self.restart_streamer_locked(app, &mut inner);
        let status = build_status(&inner);
        drop(inner);
        let _ = app.emit(STATUS_EVENT, status.clone());
        Ok(status)
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
            let result = match paths::streamer(&app) {
                Some(runner) => {
                    // Local devices are cheap to list, so a failure there is not fatal.
                    let local = stream::local_devices(&runner).unwrap_or_default();
                    stream::scan(&runner).map(|mut found| {
                        found.extend(local);
                        found
                    })
                }
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
                        let saved = inner.saved.clone();
                        let _ = controller.save(&saved);
                    }
                    Err(error) => inner.error = Some(error),
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
            let result = match paths::streamer(app) {
                Some(runner) => stream::local_devices(&runner),
                None => Err("the streamer is missing from the app".to_string()),
            };
            let mut inner = self.lock();
            if let Ok(local) = result {
                inner.available.retain(|r| !stream::is_local(&r.identifier));
                inner.available.extend(dedupe(local));
            }
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

    /// Stops the streamer and the tap. Called when the app quits. The Mac's output
    /// returns to normal once the tap has stopped.
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
    }

    /// Starts the tap if it is not running. The tap mutes the Mac's own output.
    fn ensure_tap_locked(self: &Arc<Self>, app: &AppHandle, inner: &mut Inner) {
        if inner.tap.is_some() {
            return;
        }
        let Some(tap_binary) = paths::tap_binary() else {
            inner.error = Some("the audio tap is missing from the app".into());
            return;
        };
        inner.tap_generation += 1;
        let generation = inner.tap_generation;
        let controller = Arc::clone(self);
        let exit_app = app.clone();
        match stream::start_tap(&tap_binary, move || controller.on_tap_exit(&exit_app, generation)) {
            Ok(tap) => inner.tap = Some(tap),
            Err(error) => inner.error = Some(error),
        }
    }

    /// Stops the current streamer and, if anything is selected, starts one for the selection.
    fn restart_streamer_locked(self: &Arc<Self>, app: &AppHandle, inner: &mut Inner) {
        if let Some(tap) = inner.tap.as_ref() {
            tap.detach();
        }
        if let Some(streamer) = inner.streamer.take() {
            streamer.stop();
        }
        inner.generation += 1;
        inner.links.clear();
        inner.error = None;
        if inner.saved.selected.is_empty() {
            return;
        }

        let (Some(tap), Some(runner)) = (inner.tap.as_ref(), paths::streamer(app)) else {
            inner.error = Some("the audio tap or streamer is missing from the app".into());
            return;
        };
        for receiver in &inner.saved.selected {
            inner.links.insert(
                receiver.identifier.clone(),
                Link {
                    state: "connecting".into(),
                    message: String::new(),
                },
            );
        }

        let generation = inner.generation;
        let on_line = {
            let controller = Arc::clone(self);
            let app = app.clone();
            move |line: String| controller.on_line(&app, generation, &line)
        };
        match stream::start_streamer(tap, &runner, &inner.saved.selected, on_line) {
            Ok(streamer) => inner.streamer = Some(streamer),
            Err(error) => inner.error = Some(error),
        }
    }

    fn on_line(&self, app: &AppHandle, generation: u64, line: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        if value["event"] != "receiver" {
            return;
        }
        let (Some(identifier), Some(state)) = (value["identifier"].as_str(), value["state"].as_str()) else {
            return;
        };
        let message = value["message"].as_str().unwrap_or_default().to_string();
        let status = {
            let mut inner = self.lock();
            if inner.generation != generation {
                return;
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
        });
    }
    receivers.sort_by_key(|r| r.name.to_lowercase());

    Status {
        receivers,
        active: inner.streamer.is_some(),
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
