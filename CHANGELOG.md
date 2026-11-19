# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses the versioning described in `CLAUDE.md`.

## [Unreleased]

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
