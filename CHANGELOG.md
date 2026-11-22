# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses the versioning described in `CLAUDE.md`.

## [Unreleased]

## [0.8.7] - 2026-11-22

### Added

- A *Calibrate sync* button in the popover. It plays a click track through each AirPlay receiver in turn, with the others muted, records it with the Mac's microphone, and sets a per-receiver delay so the receivers line up. A sheet explains the method and asks before the microphone is used. The recording is deleted once measured.
- The streamer can delay each receiver (`delay ID MS` on the control socket) and has `clicks` and `lag` subcommands for calibration.
- `mao-tap record SECONDS PATH` records the Mac's microphone to a WAV file.
- Microphone usage description in the app's Info.plist.

## [0.8.6] - 2026-11-22

### Fixed

- The info view keeps its text clear of the scroll bar, and long words and links wrap where needed.

## [0.8.5] - 2026-11-22

### Added

- A *Remember AirPlay receivers* switch on the info view, off by default. When on, the AirPlay receivers you tick are kept between launches, and start once the launch-time search has found them.

## [0.8.4] - 2026-11-22

### Changed

- Open at login uses the macOS login-item API on macOS 13 and later, which needs no file in `~/Library/LaunchAgents`. Older macOS, or a refusal from the API, falls back to the LaunchAgent. The README explains how to fix a root-owned LaunchAgents folder.

## [0.8.3] - 2026-11-22

### Added

- A *Notify when a receiver drops* switch on the info view, under the login switch. Notifications are off by default.

## [0.8.2] - 2026-11-22

### Added

- A macOS notification when a ticked receiver that was streaming is lost. It is sent once per drop, not on each retry.

## [0.8.1] - 2026-11-22

### Added

- A dot on the menu bar icon while any receiver is streaming.

## [0.8.0] - 2026-11-22

### Added

- Mute all: a button in the popover header, and the shortcut Option+Command+M, mute everything that is streaming. Pressing it again restores what it muted, and leaves anything muted by hand alone.

## [0.7.4] - 2026-11-22

### Fixed

- Space on the right of the receiver list, so the scroll bar no longer sits against the status text and the volume percentage.

## [0.7.3] - 2026-11-22

### Changed

- When mao searches for receivers, or the outputs on this Mac change, an output on this Mac that is no longer present is removed from the list and from the selection.

## [0.7.2] - 2026-11-22

### Added

- A volume slider and mute button for an output on this Mac, the same as for AirPlay receivers. They set that output's own volume through the tap helper. While it is the Mac's system output, that is the Mac's volume.

## [0.7.1] - 2026-11-20

### Added

- A mute button at the left of each streaming AirPlay receiver's volume slider. Muting sets the receiver's volume to zero, and unmuting restores its previous level.

## [0.7.0] - 2026-11-20

### Added

- A ticked AirPlay receiver that drops off the network, or fails to connect, is retried every 10 seconds until it returns or is unticked.

## [0.6.9] - 2026-11-20

### Added

- Licensed under Apache-2.0: `LICENSE.md`, a licence section in the README, and the licence in the Cargo and npm metadata.

## [0.6.8] - 2026-11-20

### Added

- A volume slider under each streaming AirPlay receiver, which sets that receiver's volume.

## [0.6.7] - 2026-11-20

### Changed

- `build.sh --release` builds the disk image for the version first when there is none in `dist/`, and stops if that build fails.

## [0.6.6] - 2026-11-20

### Fixed

- `build.sh --delete-release` accepts a version (`--delete-release 0.5.9`), and deletes the current version when none is given.

## [0.6.5] - 2026-11-20

### Added

- `build.sh --list-release` lists the GitHub releases of this repository.

## [0.6.4] - 2026-11-20

### Added

- A spinning indicator next to "Looking for AirPlay receivers…", so a search that is still running is visible.

## [0.6.3] - 2026-11-20

### Changed

- AirPlay receivers are no longer remembered between launches. Ticking one before discovery had finished gave an error, so they must be ticked once found. Outputs on this Mac are still remembered.

## [0.6.2] - 2026-11-20

### Changed

- At launch, the popover ticks the output on this Mac that the Mac is currently using, in place of the saved one, so the selection matches what the Mac plays through.

## [0.6.1] - 2026-11-20

### Changed

- Adding or removing an AirPlay receiver no longer restarts the streamer, so the other receivers keep streaming.

## [0.6.0] - 2026-11-20

### Changed

- AirPlay receivers start together: the streamer waits until every selected receiver has connected, or for 10 seconds at most, before it starts any of them.

## [0.5.9] - 2026-11-20

### Fixed

- `build.sh --nonotarize` no longer prints Tauri's "skipping app notarization" warning, which is expected for that build.

## [0.5.8] - 2026-11-20

### Changed

- Updated app icon (`mao.png`), menu bar and info icon (`mao-bw.png`), and the Attribution credit.

## [0.5.7] - 2026-11-20

### Added

- `./mao --version` prints the version, read from `package.json`.

## [0.5.6] - 2026-11-20

### Added

- The info view shows the Attribution section from the README, below the Help Support Development text, with a space above it.

## [0.5.5] - 2026-11-20

### Changed

- More space on the info view: between the version and the *Open mao at login* switch, and between the switch and the text below it.

## [0.5.4] - 2026-11-20

### Changed

- Outputs on this Mac: one can be ticked at a time. It becomes the Mac's system output, and macOS plays through it directly. The copying players are gone, and their Core Audio routing was not reliable.
- The Mac's own output is no longer muted when a local output is ticked. It is muted whenever only AirPlay receivers or nothing is selected, so unticked outputs stay silent.

## [0.5.3] - 2026-11-20

### Fixed

- Audio from the built-in outputs (headphones and speakers) was mixed with the Mac's microphone, which is part of those devices' input. The capture now reads only the tap's channels, so the microphone is no longer played as noise.

## [0.5.2] - 2026-11-20

### Fixed

- Headphones and speakers selected on their own played nothing. The capture helper ran alongside the local players on the same device, so it now runs only when an AirPlay receiver is selected, or when nothing is selected (to keep the Mac silent).

## [0.5.1] - 2026-11-20

### Changed

- Outputs on this Mac (speakers, headphones and other devices) are played by the tap helper, `mao-tap play <device>`, through Core Audio on the device's own clock. AirPlay receivers still go through the streamer. The Python audio library is no longer used.
- The helper lists this Mac's outputs (`mao-tap devices`), and the streamer only handles AirPlay receivers.
- Saved selections of outputs on this Mac are matched to the current device with the same name.

### Fixed

- Speakers and headphones selected in mao played nothing. The Python route's device buffer ran dry on nearly every write.

## [0.5.0] - 2026-11-20

### Fixed

- This Mac's outputs (the speakers, headphones and other devices) played nothing although they showed "streaming". The local output buffer was too small for audio arriving in real time and ran dry on nearly every write, so it now uses a larger buffer.

## [0.4.9] - 2026-11-19

### Changed

- The menu bar icon and the info view icon use the outline artwork, `mao-bw.png`. The app icon is unchanged.

## [0.4.8] - 2026-11-19

### Changed

- The app icon, the installed app's icon and the menu bar icon use the new mao artwork. The menu bar icon is a silhouette, since macOS template icons are single-colour.

## [0.4.7] - 2026-11-19

### Changed

- The streamer is frozen with PyInstaller (`scripts/build-streamer.sh`) and bundled in the app, so a released app needs no Python installed. The Python script remains the fallback when developing from the repo.

## [0.4.6] - 2026-11-19

### Added

- `build.sh` builds the Developer ID signed disk image, notarises and staples it by default, and creates or deletes the GitHub release for a version.

## [0.4.5] - 2026-11-19

### Added

- `mao` script at the repo root. `./mao` sets up what is missing, builds the app and opens it. `./mao --dev` runs the development build, and `./mao --stop` quits the running app.

## [0.4.4] - 2026-11-19

### Added

- An *Open mao at login* switch on the info view. It writes or removes a LaunchAgent in `~/Library/LaunchAgents`, and it needs the installed app.

## [0.4.3] - 2026-11-19

### Changed

- While mao runs, the Mac's own output is muted. Audio plays only on the receivers and outputs you tick, and with nothing ticked no audio plays.
- The audio tap runs for the whole session. Changing the selection restarts only the streamer.

### Fixed

- The Mac's output no longer plays alongside a copy of itself when the Mac's speakers are ticked.

## [0.4.2] - 2026-11-19

### Changed

- The info view's title reads "mao (Multi Audio/AirPlay Output)".

## [0.4.1] - 2026-11-19

### Changed

- The name and version on the info view are centred.

## [0.4.0] - 2026-11-19

### Changed

- The large icon on the info view is centred.

## [0.3.9] - 2026-11-19

### Changed

- The info view shows a large version of the mao icon above the name.

## [0.3.8] - 2026-11-19

### Added

- Web links in the info view open in the default browser.

## [0.3.7] - 2026-11-19

### Changed

- The info view shows the support text from the README's "Help Support Development" section.

### Fixed

- The info view replaces the main view completely, so the receiver list no longer shows under it.
- The Quit button sits at the bottom of the popover.

## [0.3.6] - 2026-11-19

### Added

- An info button at the right of the popover header. It replaces the window with the version and the Help, support and development section of the README. Click anywhere in it to go back.

## [0.3.5] - 2026-11-19

### Changed

- The popover header shows the mao icon before the word "mao".

## [0.3.4] - 2026-11-19

### Added

- Outputs on this Mac update as devices are plugged in or removed. mao listens for CoreAudio device-list changes (`src-tauri/src/devices.rs`), so headphones appear without restarting.

## [0.3.3] - 2026-11-19

### Added

- A Quit button at the bottom of the popover. It stops the audio helpers and exits, as the tray menu's Quit does.

## [0.3.2] - 2026-11-19

### Fixed

- The Mac's current output, such as its built-in speakers, is now listed as a target. Previously it was left out, which hid it whenever it was the default output.

## [0.3.1] - 2026-11-19

### Changed

- The menu bar icon is now the mao artwork, drawn as a template image so it follows light and dark menu bars.

## [0.3.0] - 2026-11-19

### Added

- Outputs on this Mac, such as headphones and other audio interfaces, can be ticked as targets, alongside AirPlay receivers. They are marked "this Mac".
- `mao_stream.py devices` lists them.
- `sounddevice` 0.5.6 is pinned in `python/requirements.txt`.

## [0.2.0] - 2026-11-19

### Changed

- mao now sends audio to AirPlay receivers itself, instead of through a Core Audio multi-output device. Your Mac keeps playing through its own output as before.
- Receivers are found through Bonjour and show their real names, such as "Kitchen", not a generic "AirPlay".
- Audio is captured with a Core Audio process tap, built from `tap/mao-tap.swift`.
- Streaming uses `pyatv` 0.18 in a Python environment (`scripts/setup-python.sh`), with `srptools` pinned to 1.0.1.

### Added

- `scripts/build-tap.sh` builds the audio-capture helper, and the `dev`, `build` and `test` scripts run it first.
- `scripts/setup-python.sh` creates the Python environment in `~/Library/Application Support/au.com.lateralblast.mao/venv`.
- Each receiver shows its state: connecting, streaming, ended or error, with the reason on hover.
- Selected receivers that are offline stay in the list, marked "not on the network".

### Removed

- The Core Audio multi-output device, the default-output switching and the restore-on-quit logic. None are needed now.

## [0.1.1] - 2026-11-19

### Fixed

- Selected outputs that are already inside the active route are no longer dropped when CoreAudio hides them from its device list. Previously a device-list change could rebuild the route without them.

## [0.1.0] - 2026-11-19

### Added

- Menu bar app built with Tauri 2 and Rust. It has no Dock icon.
- Click the menu bar icon to open a popover listing every audio output, with AirPlay devices marked.
- Tick outputs to send system audio to all of them at once. The selection creates a `mao Multi-Output` device and makes it the default and system-sound output.
- Restores the previous output when everything is unticked or when mao quits.
- Recovers from a previous run that did not exit cleanly.
- Keeps selected outputs that are not connected, and rejoins them when they reappear.
- Saves the selection between launches in the app's config folder.
