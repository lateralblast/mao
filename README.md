<img src="mao.png" alt="mao" width="110" align="right">

# mao

Version: 0.9.3

mao (Multi Audio/AirPlay Output) is a menu bar app for macOS. It sends your Mac's audio to several AirPlay receivers, such as HomePods, at once, and to one output on the Mac itself.

## Features

- **Several AirPlay receivers at once.** Tick any number of HomePods or other AirPlay receivers, and they play the same audio.
- **Receivers by their real names.** Discovery uses each receiver's own name, and leaves out your Mac.
- **One output on your Mac.** Tick the built-in speakers, headphones or another output, and macOS plays through it directly.
- **Silent unless ticked.** Audio plays only where you send it. With nothing ticked, the Mac is muted.
- **Mute all.** A button in the popover header, or Option+Command+M from anywhere, mutes everything that is streaming. Pressing it again restores what it muted, and leaves anything muted by hand alone.
- **Start together.** Receivers begin playing once they have all connected, or after 10 seconds at most.
- **Add and remove without interruption.** Ticking or unticking a receiver doesn't restart the others.
- **Volume keys.** When the Mac's volume changes, from the volume keys or the Sound menu, every streaming AirPlay receiver and the output on your Mac move with it, and keep their levels relative to each other.
- **Volume and mute for every streaming output.** Each AirPlay receiver, and each output on your Mac, has a volume slider and a mute button. Muting keeps the level for when you unmute.
- **Reconnects by itself.** A ticked receiver that drops off the network is retried every 10 seconds. A macOS notification can say when one drops, if you turn that on in the info view (off by default).
- **Remembers your outputs.** The outputs on your Mac you choose are saved. AirPlay receivers can be remembered too, with the *Remember AirPlay receivers* switch in the info view (off by default). Remembered receivers start once the launch-time search has found them.
- **Follows the Mac's output at launch.** The output the Mac is using is ticked when mao starts.
- **Calibrate sync.** Lines up the receivers with a click track, measured with your Mac's microphone. It asks before it uses the microphone.
- **Shows when it is searching.** A spinner appears while receivers are being looked for.
- **Finds devices as they change.** Receivers and outputs appear and disappear as you plug in or switch them off, without restarting mao. An output on your Mac that is no longer present is removed from the selection.
- **Menu bar app.** It has no Dock icon, and it can open at login from the info view. A dot on the icon shows while anything is streaming.
- **Info view.** It shows the version, the help text, and the attributions.
- **Self-contained.** The streamer is bundled, so the app needs no Python installed.

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

- [Features](#features)
- [Quick start](#quick-start)
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

<p align="center">
  <img src="docs/mao-info.png" alt="The mao info view, with the Calibrate sync button at the bottom" width="300">
  <br>
  <em>The info view. Calibrate sync is at the bottom, outside the scrolling text.</em>
</p>

Click the mao icon in the menu bar. The popover lists the AirPlay receivers on your network and the outputs on your Mac (marked *your Mac*). Tick a receiver to send audio to it, and untick it to stop. AirPlay receivers are forgotten when mao quits, unless you turn on *Remember AirPlay receivers* in the info view. Outputs on your Mac are remembered.

- **AirPlay receivers** can be ticked in any number, together with one output on your Mac.
- **One output on your Mac** can be ticked at a time. Ticking another one unticks the first.
- Devices appear and disappear as you plug them in or unplug them. AirPlay receivers are searched for each time the popover opens.
- Each ticked receiver shows its state: connecting, streaming, ended or error. Hover over an error for the reason.
- A streaming AirPlay receiver has a volume slider under its name, with a mute button at its left. Muting sets that receiver's own volume to zero, and unmuting restores the level it had. It does not change the Mac's volume.
- An output on your Mac has the same volume slider and mute button. They change that output's own volume. While that output is the Mac's system output, this is the Mac's volume.
- At launch, the output on your Mac that the Mac is currently using is ticked, so the popover matches what the Mac plays through.
- **Calibrate sync** (in the info view, opened with the info button in the popover header) lines up the AirPlay receivers. A sheet explains the method first. It needs your OK, because it uses the Mac's microphone. Then mao plays a click track through each receiver in turn, with the others muted, and records it for about 45 seconds per receiver. It sets a delay for each receiver from what it hears, and deletes the recording once measured. It needs at least two AirPlay receivers streaming, and no output on your Mac ticked. Keep the room quiet while it runs.
- **Volume keys:** when the Mac's volume changes (the volume keys or the Sound menu), every streaming AirPlay receiver is scaled by the same ratio, so the receivers move together and keep their levels relative to each other.
- Quit mao with the button at the bottom of the popover.

## How it works

- `tap/mao-tap.swift` is a Core Audio helper. It copies the audio your Mac plays, and it sets the Mac's system output when an output on your Mac is ticked.
- When an AirPlay receiver is ticked, or nothing is ticked, the capture mutes the Mac's own output. Nothing plays on your Mac unless you tick an output on it.
- When an output on your Mac is ticked, it becomes the Mac's system output and macOS plays through it directly. mao restores the previous output when that output is unticked, and when mao quits.
- `python/mao_stream.py` finds AirPlay receivers with `pyatv` and streams the copied audio to each one. It sends silence when nothing is playing, so receivers stay connected between sounds. It is frozen into the app as `mao-stream`.
- The Rust app in `src-tauri/` runs the helpers, keeps the saved selection, and shows the popover.

## Limitations

- **One output on your Mac at a time.** The Mac has one system output, so mao can't play on two outputs on your Mac at once. AirPlay receivers can be combined with it.
- **Receivers drift apart.** They start together, but each has its own playback buffer. Calibrate sync measures each receiver's offset and delays it to match. The delays are set once, so they don't follow drift over a long session; run calibration again if the receivers fall out of step.
- **A dropped receiver is retried every 10 seconds** while it is ticked. If it stays away, it shows an error until it returns or you untick it.
- **AirPlay receivers are not remembered by default.** Turn on *Remember AirPlay receivers* in the info view to keep them. Without it, tick them once they have been found.
- **Volume and mute need a streaming receiver.** An AirPlay receiver's slider appears only while it is streaming. An output on your Mac needs a device that has a volume or mute control, and otherwise has no slider.
- **The Mac's volume is followed only while the output on your Mac is not muted by mao.** When nothing is ticked on your Mac, mao mutes the built-in output, so the volume keys don't scale the receivers then.
- **An output on your Mac is the Mac's volume.** While it is the system output, its slider changes the Mac's volume.
- **Ticking or unticking an output on your Mac restarts the capture.** This can cause a short gap in AirPlay audio.
- **The Mac's output is muted unless an output on your Mac is ticked.** Quit mao to get normal playback back.
- **The original output may not be restored.** mao restores the output it changed, but it can't tell what the output was if it was already the device you first ticked.
- **The streamer starts a little slower** than a plain Python script, because PyInstaller unpacks it on each start.
- **AirPlay depends on `pyatv`.** It speaks a protocol Apple doesn't publish, so an Apple update could break it. `pyatv` and `srptools` are pinned in `python/requirements.txt`.
- **macOS only.**

## Open at login

The info view has an *Open mao at login* switch. How it works depends on your macOS version:

- **macOS 13 and later:** mao registers itself as a login item, using the system's login-item API. It shows under Login Items in System Settings, where you can also switch it off. Nothing is written to your home folder.
- **Older macOS, or if the login-item API refuses:** mao writes a LaunchAgent to `~/Library/LaunchAgents/au.com.lateralblast.mao.plist`, which opens the installed app quietly at login.

The switch needs the installed app, so it is disabled when running from a build directory.

### If the login switch shows "Permission denied"

This happens with the LaunchAgent method when `~/Library/LaunchAgents` is owned by root. Some installers create that folder as root. To give your own user ownership of it, run this in Terminal, and enter your password when asked:

```sh
sudo chown "$(whoami)" ~/Library/LaunchAgents
```

Then turn the switch on again. Alternatively, use macOS 13 or later, where the login-item method doesn't need that folder.

## Project layout

- `src/`: the popover interface (HTML, CSS and JavaScript, with no build step).
- `src-tauri/src/lib.rs`: the menu bar icon, the popover window and the Tauri commands.
- `src-tauri/src/controller.rs`: the saved selection, the states of receivers and outputs, and routing.
- `src-tauri/src/stream.rs`: runs the tap and the streamer, and sets the system output.
- `src-tauri/src/devices.rs`: listens for device changes.
- `src-tauri/src/volume_follow.rs`: follows the Mac's volume and scales the receivers with it.
- `src-tauri/src/calibrate.rs`: the sync calibration: the click track, the measurement and the delays.
- `src-tauri/src/login.rs`: open at login (the login-item API, or a LaunchAgent on older macOS).
- `src-tauri/src/paths.rs`: finds the helpers, both in a built app and in the repo.
- `tap/mao-tap.swift`: the audio capture and output helper.
- `python/mao_stream.py`: AirPlay discovery and streaming, with tests in `python/test_mao_stream.py`.
- `scripts/`: build and setup scripts.

## Help Support Development

If you find this software useful and would like to support its development, please consider buying me a coffee:

https://ko-fi.com/richardatlateralblast

## Attribution

Hamster icons created by [MEDZ - Flaticon](https://www.flaticon.com/free-icons/hamster)

## Licence

Copyright 2026 Richard Spindler. Licensed under the Apache License, Version 2.0. See [LICENSE.md](LICENSE.md).
