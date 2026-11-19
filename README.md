# mao

Version: 0.4.9

mao (Multi Audio/AirPlay Output) is a menu bar app for macOS that sends your system audio to several AirPlay receivers, such as HomePods, at once, while your Mac keeps playing through its own speakers.

## Quick start

Requirements: macOS 14.2 or later (for the system-audio tap), Xcode command line tools (for `swiftc`), [Rust](https://rustup.rs), Node.js with npm, and Homebrew Python 3.12.

```sh
./mao                # set up what is missing, build the app and open it
./mao --dev          # run the development build
./mao --stop         # quit the running app
```

The lower-level steps are `npm install`, `scripts/setup-python.sh` (the Python environment for streaming), `npm run dev`, `npm run build`, and `npm test` (the Rust and Python tests).

The first time mao captures audio, macOS asks for permission to capture system audio. Allow it, or nothing is sent.

## Contents

- [Using mao](#using-mao)
- [How it works](#how-it-works)
- [Limitations](#limitations)
- [Project layout](#project-layout)
- [Help Support Development](#help-support-development)
- [Changelog](CHANGELOG.md)

## Using mao

Click the mao icon in the menu bar. Quit mao from the button at the bottom of the popover. The popover lists the AirPlay receivers on your network, and the outputs on this Mac (marked *this Mac*). Devices appear and disappear as you plug them in or unplug them, without restarting mao. AirPlay receivers are rediscovered when the popover opens. Tick one to send audio to it, and untick it to stop. Your selection is saved and resumed on the next launch.

Each ticked receiver shows its state: connecting, streaming, ended or error. Hover over an error for the reason.

## How it works

- `tap/mao-tap.swift` uses a Core Audio process tap to copy the audio your Mac plays. While mao runs, the tap mutes the Mac's own output, so audio plays only on the devices you tick. With nothing ticked, nothing plays.
- `python/mao_stream.py` finds receivers with `pyatv` and streams the copied audio to each one. It sends silence whenever nothing is playing, so receivers stay connected between sounds.
- The Rust app (`src-tauri/`) runs both helpers, keeps the saved selection, and shows the menu bar popover.

## Limitations

- Receivers are not kept in step with each other. Each has its own buffer, so they can drift apart, and sync between them has not been measured yet.
- Changing the selection restarts the streamer. Receivers already playing drop out for a moment.
- If a receiver drops off the network mid-stream, it stays in the error state until you untick and retick it.
- While mao runs, the Mac's own output is muted. Quit mao to get normal playback back.
- The bundled streamer starts a little slower than a plain Python script, because PyInstaller unpacks it on each start.
- Outputs on this Mac are played from a separate copy of the audio, so they can drift slightly from the default output.
- Apple's private AirPlay code is not used. `pyatv` talks to receivers over AirPlay, so an Apple update could break it. Pin `pyatv` and `srptools` in `python/requirements.txt`.
- macOS only.

## Open at login

The info view has an *Open mao at login* switch. It works by writing a LaunchAgent to `~/Library/LaunchAgents/au.com.lateralblast.mao.plist`, which opens the installed app quietly at login. The switch needs the installed app, so it is disabled when running from a build directory.

## Help Support Development

If you find this software useful and would like to support its development, please consider buying me a coffee:

https://ko-fi.com/richardatlateralblast
