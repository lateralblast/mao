<img src="mao.png" alt="mao" width="110" align="right">

# mao

Version: 0.5.9

mao (Multi Audio/AirPlay Output) is a menu bar app for macOS. It sends your Mac's audio to several AirPlay receivers, such as HomePods, at once, and to one output on the Mac itself.

## Quick start

<p align="center">
  <img src="docs/mao-installer.png" alt="The mao disk image: drag mao into Applications" width="360">
  <br>
  <em>The disk image. Drag mao into Applications.</em>
</p>

Requirements: macOS 14.2 or later (for the system-audio tap), Xcode command line tools (for `swiftc`), [Rust](https://rustup.rs), Node.js with npm, and Homebrew Python 3.12.

```sh
./mao                # set up what is missing, build the app and open it
./mao --dev          # run the development build
./mao --stop         # quit the running app
./mao --version      # print the version
```

The lower-level steps are `npm install`, `scripts/setup-python.sh` (the Python environment used to build the streamer), `npm run dev`, `npm run build`, and `npm test` (the Rust and Python tests).

The first time mao captures audio, macOS asks for permission to capture system audio. Allow it, or nothing is sent.

## Contents

- [Using mao](#using-mao)
- [How it works](#how-it-works)
- [Limitations](#limitations)
- [Open at login](#open-at-login)
- [Project layout](#project-layout)
- [Help Support Development](#help-support-development)
- [Changelog](CHANGELOG.md)

## Using mao

<p align="center">
  <img src="docs/mao-popover.png" alt="The mao popover, with placeholder device names" width="300">
  <br>
  <em>The popover. The device names are placeholders.</em>
</p>

Click the mao icon in the menu bar. The popover lists the AirPlay receivers on your network and the outputs on this Mac (marked *this Mac*). Tick a receiver to send audio to it, and untick it to stop. Your selection is saved and resumed on the next launch.

- **AirPlay receivers** can be ticked in any number, together with one output on this Mac.
- **One output on this Mac** can be ticked at a time. Ticking another one unticks the first.
- Devices appear and disappear as you plug them in or unplug them. AirPlay receivers are searched for each time the popover opens.
- Each ticked receiver shows its state: connecting, streaming, ended or error. Hover over an error for the reason.
- Quit mao with the button at the bottom of the popover.

## How it works

- `tap/mao-tap.swift` is a Core Audio helper. It copies the audio your Mac plays, and it sets the Mac's system output when an output on this Mac is ticked.
- When an AirPlay receiver is ticked, or nothing is ticked, the capture mutes the Mac's own output. Nothing plays on this Mac unless you tick an output on it.
- When an output on this Mac is ticked, it becomes the Mac's system output and macOS plays through it directly. mao restores the previous output when that output is unticked, and when mao quits.
- `python/mao_stream.py` finds AirPlay receivers with `pyatv` and streams the copied audio to each one. It sends silence when nothing is playing, so receivers stay connected between sounds. It is frozen into the app as `mao-stream`.
- The Rust app in `src-tauri/` runs the helpers, keeps the saved selection, and shows the popover.

## Limitations

- **One output on this Mac at a time.** The Mac can have only one system output, so mao can't play on two outputs on this Mac at once. AirPlay receivers can be combined with it.
- Receivers are not kept in step with each other. Each has its own buffer, so they can drift apart, and sync between them has not been measured.
- Changing the selection restarts the streamer, so receivers already playing drop out for a moment.
- If a receiver drops off the network mid-stream, it stays in the error state until you untick and retick it.
- While nothing on this Mac is ticked, the Mac's own output is muted. Quit mao to get normal playback back.
- mao can only restore the output it changed if it knows what that output was. If the Mac's output was already the device you first ticked, mao cannot restore a different one.
- The streamer starts a little slower than a plain Python script, because PyInstaller unpacks it on each start.
- `pyatv` talks to AirPlay receivers over a protocol Apple doesn't publish, so an Apple update could break it. `pyatv` and `srptools` are pinned in `python/requirements.txt`.
- macOS only.

## Open at login

The info view has an *Open mao at login* switch. It writes a LaunchAgent to `~/Library/LaunchAgents/au.com.lateralblast.mao.plist`, which opens the installed app quietly at login. The switch needs the installed app, so it is disabled when running from a build directory.

## Project layout

- `src/`: the popover interface (HTML, CSS and JavaScript, with no build step).
- `src-tauri/src/lib.rs`: the menu bar icon, the popover window and the Tauri commands.
- `src-tauri/src/controller.rs`: the saved selection, the states of receivers and outputs, and routing.
- `src-tauri/src/stream.rs`: runs the tap and the streamer, and sets the system output.
- `src-tauri/src/devices.rs`: listens for device changes.
- `src-tauri/src/paths.rs`: finds the helpers, both in a built app and in the repo.
- `tap/mao-tap.swift`: the audio capture and output helper.
- `python/mao_stream.py`: AirPlay discovery and streaming, with tests in `python/test_mao_stream.py`.
- `scripts/`: build and setup scripts.

## Help Support Development

If you find this software useful and would like to support its development, please consider buying me a coffee:

https://ko-fi.com/richardatlateralblast

## Attribution

Hamster icons created by [MEDZ - Flaticon](https://www.flaticon.com/free-icons/hamster)