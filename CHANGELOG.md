# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses the versioning described in `CLAUDE.md`.

## [Unreleased]

## [0.3.0] - 2026-11-19

### Added

- Outputs on this Mac, such as headphones and other audio interfaces, can be ticked as targets, alongside AirPlay receivers. They are marked "this Mac".
- `mao_stream.py devices` lists them. The Mac's current default output is left out, because it already plays the audio.
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
