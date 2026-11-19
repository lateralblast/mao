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

## Help Support Development

If you find this software useful and would like to support its development, please consider buying me a coffee:

https://ko-fi.com/richardatlateralblast

## Releases

`./build.sh` builds `dist/mao_<version>_<arch>.dmg`, signed with the Developer ID certificate and notarised and stapled by default:

```sh
./build.sh --check          # show the signing identity, notary credentials and tools
./build.sh                  # sign and notarise
./build.sh --nonotarize     # sign only
./build.sh --unsigned       # for local tests
./build.sh --release        # tag v<version>, push it and create the GitHub release with the disk image
./build.sh --delete-release # delete the GitHub release and its tag
```

Notarisation needs an App Store Connect API key (`APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_PATH`) or an Apple ID (`APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`). They come from the environment or from `~/.config/mao/notary.env`, and `~/.config/cheetoh/notary.env` is used if that is missing. `--release` needs the work committed and pushed, and `gh` logged in. Use `--dry-run` or `--draft` to check a release first.

The app bundles its own Python: `scripts/build-streamer.sh` freezes the streamer with PyInstaller, so a released app needs no Python on the Mac it runs on.

## Open at login

The info view has an *Open mao at login* switch. It works by writing a LaunchAgent to `~/Library/LaunchAgents/au.com.lateralblast.mao.plist`, which opens the installed app quietly at login. The switch needs the installed app, so it is disabled when running from a build directory.

## Project layout

- `src/`: the popover interface (HTML, CSS and JavaScript, with no build step).
- `src-tauri/src/lib.rs`: the menu bar icon, the popover window and the Tauri commands.
- `src-tauri/src/controller.rs`: the saved selection, receiver states and restarting the stream.
- `src-tauri/src/stream.rs`: runs the tap and the streamer, and connects them.
- `src-tauri/src/paths.rs`: finds the helpers, both in a built app and in the repo.
- `tap/mao-tap.swift`: the system-audio capture helper.
- `python/mao_stream.py`: discovery and streaming, with tests in `python/test_mao_stream.py`.
- `scripts/`: build and setup scripts.

## Help Support Development

If you find this software useful and would like to support its development, please consider buying me a coffee:

https://ko-fi.com/richardatlateralblast
