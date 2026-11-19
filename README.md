# mao

Version: 0.3.5

mao (Multi AirPlay Output) is a menu bar app for macOS that sends your system audio to several AirPlay receivers, such as HomePods, at once, while your Mac keeps playing through its own speakers.

## Quick start

Requirements: macOS 14.2 or later (for the system-audio tap), Xcode command line tools (for `swiftc`), [Rust](https://rustup.rs), Node.js with npm, and Homebrew Python 3.12.

```sh
npm install
scripts/setup-python.sh    # creates the Python environment for streaming
npm run dev                # build the tap helper and run the app
npm run build              # build the app bundle
npm test                   # run the Rust and Python tests
```

The first time mao captures audio, macOS asks for permission to capture system audio. Allow it, or nothing is sent.

## Contents

- [Using mao](#using-mao)
- [How it works](#how-it-works)
- [Limitations](#limitations)
- [Project layout](#project-layout)
- [Changelog](CHANGELOG.md)

## Using mao

Click the mao icon in the menu bar. Quit mao from the button at the bottom of the popover. The popover lists the AirPlay receivers on your network, and the outputs on this Mac (marked *this Mac*). Devices appear and disappear as you plug them in or unplug them, without restarting mao. AirPlay receivers are rediscovered when the popover opens. Tick one to send audio to it, and untick it to stop. Your selection is saved and resumed on the next launch.

Each ticked receiver shows its state: connecting, streaming, ended or error. Hover over an error for the reason.

## How it works

- `tap/mao-tap.swift` uses a Core Audio process tap to copy the audio your Mac plays. The tap leaves the normal output alone, so your speakers keep playing.
- `python/mao_stream.py` finds receivers with `pyatv` and streams the copied audio to each one. It sends silence whenever nothing is playing, so receivers stay connected between sounds.
- The Rust app (`src-tauri/`) runs both helpers, keeps the saved selection, and shows the menu bar popover.

## Limitations

- Receivers are not kept in step with each other. Each has its own buffer, so they can drift apart, and sync between them has not been measured yet.
- Changing the selection restarts the stream. Receivers already playing drop out for a moment.
- If a receiver drops off the network mid-stream, it stays in the error state until you untick and retick it.
- The Python environment is found through `MAO_PYTHON` or the path that `scripts/setup-python.sh` creates. A distributable bundle does not include Python yet.
- If the Mac's current output is also a target, the audio plays there twice, slightly offset. Untick it, or change the default output, to avoid the echo.
- Outputs on this Mac are played from a separate copy of the audio, so they can drift slightly from the default output.
- Apple's private AirPlay code is not used. `pyatv` talks to receivers over AirPlay, so an Apple update could break it. Pin `pyatv` and `srptools` in `python/requirements.txt`.
- macOS only.

## Project layout

- `src/`: the popover interface (HTML, CSS and JavaScript, with no build step).
- `src-tauri/src/lib.rs`: the menu bar icon, the popover window and the Tauri commands.
- `src-tauri/src/controller.rs`: the saved selection, receiver states and restarting the stream.
- `src-tauri/src/stream.rs`: runs the tap and the streamer, and connects them.
- `src-tauri/src/paths.rs`: finds the helpers, both in a built app and in the repo.
- `tap/mao-tap.swift`: the system-audio capture helper.
- `python/mao_stream.py`: discovery and streaming, with tests in `python/test_mao_stream.py`.
- `scripts/`: build and setup scripts.
