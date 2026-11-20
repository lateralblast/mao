# TODO

Status: `[ ]` open, `[~]` in progress, `[x]` done.

## Open

- [ ] Measure sync between receivers. Each has its own buffer, so they can drift apart. Plan: put Orange and White HomePods in one room, tick both, play a click track, and record about 10 s from the Mac microphone. Each click appears once per receiver, so the gap between paired clicks is the drift. Needs both receivers in one room and permission to use the microphone.
- [ ] Reconnect a receiver that drops off the network mid-stream. Today it stays in the error state until it is unticked and reticked.
- [ ] Test the popover by clicking through it on a real Mac. So far only the Rust and Python paths have run, not the UI.
- [ ] Bundle a Python runtime with the app (for example PyInstaller), so it runs without `scripts/setup-python.sh`.
- [ ] Check the audio-capture permission flow on a clean account, and what happens if the user denies it.
- [ ] Add launch at login.
- [ ] Choose a licence. `LICENSE.md` is not in the repo yet, so the package has no licence statement.

## Done

- [x] Add or remove a receiver without restarting the stream (0.6.1): the streamer takes receivers over a control socket.
- [x] Discover AirPlay receivers by name with `pyatv` (Bonjour), and skip this Mac's own addresses.
- [x] Capture system audio with a Core Audio process tap (`tap/mao-tap.swift`), which leaves the local output alone.
- [x] Stream to a HomePod with `pyatv` over AirPlay. Confirmed with Kitchen, including a click track through the full pipeline.
- [x] Pace the stream in real time, with silence while nothing plays, so receivers stay connected.
- [x] Resume the saved selection at launch, and stop the helpers when mao quits.
- [x] Remove the Core Audio multi-output device and the default-output switching. They could not reach AirPlay receivers on this macOS version.
