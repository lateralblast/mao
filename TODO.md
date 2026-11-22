# TODO

Status: `[ ]` open, `[~]` in progress, `[x]` done.

## Open

- [ ] Measure sync between receivers. Measured: Kitchen plays about 0.15 s after Front Bedroom (single-receiver runs, 4.48 s vs 4.33 s delay; the pair run gave 0.17 s). Kitchen's match was weaker, so treat 0.15 s as an estimate. To fix: add a per-receiver delay in the streamer (hold Front Bedroom back about 150 ms), then measure again. Method: irregular click track, recorded with the AVAudioEngine recorder (ffmpeg dropped samples), cross-correlated against the known click times.
- [ ] Check the audio-capture permission flow on a clean account, and what happens if the user denies it.
- [ ] Per-receiver delay: hold each receiver back by its own amount, so receivers line up. Fixes the measured offset (Kitchen about 0.15 s behind Front Bedroom).
- [ ] Built-in sync check: a popover button that runs the click-track measurement and shows the result.
- [ ] Tray icon state: a small dot on the menu bar icon while anything is streaming.
- [ ] Notify when a ticked receiver is lost and retried, rather than showing it only in the popover.
- [ ] Remember AirPlay receivers between launches, once discovery has finished.
- [ ] Presets: save a set of receivers and outputs, and switch between them.
- [ ] Several outputs on this Mac at once. The Mac has one system output, so this needs a different approach, such as an aggregate device, and it brings back the clock problems seen earlier.
- [ ] Volume that follows the Mac's volume keys. May not be possible through pyatv.

## Done

- [x] Mute all, with a global keyboard shortcut (0.8.0): a header button, and Option+Command+M.
- [x] Reconnect a receiver that drops off the network (0.7.0): a ticked receiver is retried every 10 seconds until it returns or is unticked.
- [x] Test the popover by clicking through it on a real Mac: tested throughout on a real Mac.
- [x] Bundle a Python runtime (0.4.7): the streamer is frozen with PyInstaller, so the app needs no Python installed.
- [x] Add launch at login (0.4.4): the info view has an *Open mao at login* switch, which writes a LaunchAgent.
- [x] Choose a licence: Apache-2.0 (`LICENSE.md`, the README licence section, and the Cargo and npm metadata).
- [x] Add or remove a receiver without restarting the stream (0.6.1): the streamer takes receivers over a control socket.
- [x] Discover AirPlay receivers by name with `pyatv` (Bonjour), and skip this Mac's own addresses.
- [x] Capture system audio with a Core Audio process tap (`tap/mao-tap.swift`), which leaves the local output alone.
- [x] Stream to a HomePod with `pyatv` over AirPlay. Confirmed with Kitchen, including a click track through the full pipeline.
- [x] Pace the stream in real time, with silence while nothing plays, so receivers stay connected.
- [x] Resume the saved selection at launch, and stop the helpers when mao quits.
- [x] Remove the Core Audio multi-output device and the default-output switching. They could not reach AirPlay receivers on this macOS version.
