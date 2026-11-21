# TODO

Status: `[ ]` open, `[~]` in progress, `[x]` done.

## Open

- [ ] Measure sync between receivers. Measured: Kitchen plays about 0.15 s after Front Bedroom (single-receiver runs, 4.48 s vs 4.33 s delay; the pair run gave 0.17 s). Kitchen's match was weaker, so treat 0.15 s as an estimate. To fix: add a per-receiver delay in the streamer (hold Front Bedroom back about 150 ms), then measure again. Method: irregular click track, recorded with the AVAudioEngine recorder (ffmpeg dropped samples), cross-correlated against the known click times.
- [ ] Check the audio-capture permission flow on a clean account, and what happens if the user denies it.


## Done

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
