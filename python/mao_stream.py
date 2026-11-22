#!/usr/bin/env python3
"""mao-stream: send system audio to AirPlay receivers.

    mao_stream.py scan
        Print the AirPlay receivers on the local network as a JSON list.

    mao_stream.py stream [--control SOCKET] [IDENTIFIER=ADDRESS ...]
        Read 48 kHz, 16-bit, stereo PCM from stdin and stream it to each AirPlay
        receiver. The receivers can change while it runs: a line sent to the
        control socket, "add IDENTIFIER ADDRESS", "remove IDENTIFIER" or
        "volume IDENTIFIER LEVEL" (0-100), adds, removes or sets the volume of one.
        Exits when stdin closes.

Progress goes to stdout as one JSON object per line, for example:
    {"event": "receiver", "identifier": "...", "state": "streaming", "message": ""}
"""

import array
import asyncio
import io
import json
import os
import queue
import socket
import struct
import sys
import threading
import time
import wave

import pyatv
from pyatv.const import Protocol

RATE = 48000
CHANNELS = 2
BYTES_PER_FRAME = CHANNELS * 2
TICK_SECONDS = 0.02
TICK_BYTES = int(RATE * BYTES_PER_FRAME * TICK_SECONDS)  # 20 ms = 3840 bytes
MAX_PENDING_BYTES = RATE * BYTES_PER_FRAME // 4  # keep at most 250 ms of backlog
RECEIVER_QUEUE_TICKS = 500  # about 10 s of audio per receiver before dropping, to allow for a delay
SCAN_TIMEOUT = 6
HOST_TIMEOUT = 3
# Longest wait for every receiver to connect before they all start together.
START_TIMEOUT = 10
# How long to wait before trying a receiver again after it drops or fails to connect.
RECONNECT_SECONDS = 10
SILENCE = bytes(TICK_BYTES)

_print_lock = threading.Lock()


def emit(event, **fields):
    with _print_lock:
        print(json.dumps({"event": event, **fields}), flush=True)


def describe(error):
    """The error message plus any underlying cause, since pyatv wraps errors."""
    parts = [f"{type(error).__name__}: {error}"]
    cause = error.__cause__
    while cause is not None:
        parts.append(f"{type(cause).__name__}: {cause}")
        cause = cause.__cause__
    return " <- ".join(parts)


def take_tick(pending):
    """Remove and return the next 20 ms of audio from `pending`.

    While the tap has not yet filled a whole tick, return silence and keep
    what is pending. A receiver therefore never runs dry while nothing plays.
    """
    if len(pending) > MAX_PENDING_BYTES:
        excess = len(pending) - MAX_PENDING_BYTES
        del pending[: excess + (-excess) % BYTES_PER_FRAME]  # drop whole frames only
    if len(pending) >= TICK_BYTES:
        chunk = bytes(pending[:TICK_BYTES])
        del pending[:TICK_BYTES]
        return chunk
    return SILENCE


def wav_header():
    """A WAV header for an unending stream. The data size is set high, not to the
    maximum, because the decoder rejects 0xFFFFFFFF."""
    byte_rate = RATE * BYTES_PER_FRAME
    return (
        b"RIFF"
        + struct.pack("<I", 0x7FFFF000)
        + b"WAVE"
        + b"fmt "
        + struct.pack("<IHHIIHH", 16, 1, CHANNELS, RATE, byte_rate, BYTES_PER_FRAME, 16)
        + b"data"
        + struct.pack("<I", 0x7FFFF000)
    )


def write_all(fd, data):
    view = memoryview(data)
    while view:
        view = view[os.write(fd, view) :]


EXIT_GRACE_SECONDS = 3


def read_stdin(source, on_eof):
    while True:
        data = os.read(0, 65536)
        if not data:
            source.put(None)
            on_eof()
            # The caller has gone. Do not wait on receivers that may never finish.
            threading.Timer(EXIT_GRACE_SECONDS, os._exit, [0]).start()
            return
        source.put(data)


def pace(source, current_outputs):
    """Feed every receiver one tick of audio every 20 ms, in real time.

    `current_outputs` returns the queues of the receivers at that moment, so receivers can be added
    and removed while this runs."""
    pending = bytearray()
    deadline = time.monotonic()
    finished = False
    while not finished:
        while True:
            try:
                item = source.get_nowait()
            except queue.Empty:
                break
            if item is None:
                finished = True
                break
            pending += item
        chunk = take_tick(pending)
        for out in current_outputs():
            try:
                out.put_nowait((time.monotonic(), chunk))
            except queue.Full:
                pass  # this receiver is behind: drop audio rather than stall the others
        deadline += TICK_SECONDS
        delay = deadline - time.monotonic()
        if delay > 0:
            time.sleep(delay)
        else:
            deadline = time.monotonic()  # fell behind: resynchronise the clock
    for out in current_outputs():
        out.put(None)


def write_receiver(out, fd, ready, delay_seconds):
    """Write the header, then forward audio once the receiver is connected.

    Each chunk is held back by the receiver's delay (`delay_seconds()`), which lines receivers up
    when some play later than others. Audio queued before the connection is ready is discarded, so
    the receiver starts with what is playing now rather than what was queued earlier."""
    try:
        write_all(fd, wav_header())
        while True:
            item = out.get()
            if item is None:
                break
            queued_at, chunk = item
            wait = queued_at + delay_seconds() - time.monotonic()
            if wait > 0:
                time.sleep(wait)
            if ready.is_set():
                write_all(fd, chunk)
    except OSError:
        pass  # the receiver has disconnected
    finally:
        os.close(fd)


class StartGate:
    """Opens once every receiver has connected or failed, so they all start together."""

    def __init__(self, count):
        self.open = asyncio.Event()
        self.waiting = count
        if count == 0:
            self.open.set()

    def arrived(self):
        self.waiting -= 1
        if self.waiting <= 0:
            self.open.set()


async def open_after(gate, seconds):
    await asyncio.sleep(seconds)
    gate.open.set()


async def reconnect(hub, identifier, address):
    """If the receiver is still ticked, end its stream and try again after a pause."""
    if hub is None or not hub.wants(identifier):
        return
    emit("receiver", identifier=identifier, state="error",
         message=f"lost; trying again in {RECONNECT_SECONDS} s")
    hub.drop_stream(identifier)
    await asyncio.sleep(RECONNECT_SECONDS)
    if hub.wants(identifier):
        hub.add(identifier, address)


async def stream_receiver(loop, identifier, address, reader, ready, gate, hub=None):
    emit("receiver", identifier=identifier, state="connecting", message="")
    try:
        found = await pyatv.scan(loop, timeout=HOST_TIMEOUT, hosts=[address])
        conf = next((c for c in found if c.identifier == identifier), None)
        if conf is None:
            raise RuntimeError(f"no receiver with identifier {identifier} at {address}")
        atv = await pyatv.connect(conf, loop, protocol=Protocol.AirPlay)
    except Exception as error:
        gate.arrived()
        emit("receiver", identifier=identifier, state="error", message=describe(error))
        await reconnect(hub, identifier, address)
        return

    gate.arrived()
    await gate.open.wait()
    ready.set()
    if hub is not None:
        hub.clients[identifier] = atv
    emit("receiver", identifier=identifier, state="streaming", message="")
    try:
        emit("volume", identifier=identifier, level=atv.audio.volume)
    except Exception:
        pass  # this receiver does not report its volume
    try:
        await atv.stream.stream_file(reader)
        emit("receiver", identifier=identifier, state="ended", message="")
    except Exception as error:
        emit("receiver", identifier=identifier, state="error", message=describe(error))
    finally:
        if hub is not None:
            hub.clients.pop(identifier, None)
        atv.close()
    await reconnect(hub, identifier, address)


class Hub:
    """The receivers that are streaming now. Runs in the streamer's event loop thread."""

    def __init__(self, loop):
        self.loop = loop
        self.lock = threading.Lock()
        self.outputs = {}  # identifier -> the queue the pacer fills for it
        self.clients = {}  # identifier -> the connected pyatv device, while it is streaming
        self.wanted = {}  # identifier -> address, for every receiver the user has ticked
        self.delays = {}  # identifier -> seconds each receiver is held back, to line receivers up

    def set_delay(self, identifier, milliseconds):
        """Hold a receiver back by `milliseconds`, from now on."""
        self.delays[identifier] = max(0.0, min(5000.0, float(milliseconds))) / 1000

    def set_volume(self, identifier, level):
        """Set the volume of a streaming receiver, from 0 to 100."""
        client = self.clients.get(identifier)
        if client is None:
            return
        level = max(0.0, min(100.0, float(level)))

        async def apply():
            await client.audio.set_volume(level)
            emit("volume", identifier=identifier, level=level)

        asyncio.run_coroutine_threadsafe(apply(), self.loop)

    def current_outputs(self):
        with self.lock:
            return list(self.outputs.values())

    def wants(self, identifier):
        with self.lock:
            return identifier in self.wanted

    def add(self, identifier, address, gate=None):
        with self.lock:
            self.wanted[identifier] = address
            if identifier in self.outputs:
                return
        read_fd, write_fd = os.pipe()
        reader = io.BufferedReader(io.FileIO(read_fd, "rb"))
        out = queue.Queue(maxsize=RECEIVER_QUEUE_TICKS)
        ready = threading.Event()
        threading.Thread(
            target=write_receiver,
            args=(out, write_fd, ready, lambda: self.delays.get(identifier, 0.0)),
            daemon=True,
        ).start()
        with self.lock:
            self.outputs[identifier] = out
        # A receiver added later does not wait for the others: the start gate is already open.
        gate = gate or StartGate(0)
        asyncio.run_coroutine_threadsafe(
            stream_receiver(self.loop, identifier, address, reader, ready, gate, self), self.loop
        )

    def remove(self, identifier):
        with self.lock:
            self.wanted.pop(identifier, None)
        self.drop_stream(identifier)

    def drop_stream(self, identifier):
        """End the stream for `identifier`, keeping it wanted (so it can be started again)."""
        with self.lock:
            out = self.outputs.pop(identifier, None)
        if out is not None:
            # Ending the queue ends the writer, which closes the pipe, which ends the stream.
            threading.Thread(target=out.put, args=(None,), daemon=True).start()


def serve_control(path, hub):
    """Accept "add IDENTIFIER ADDRESS" and "remove IDENTIFIER" lines on a Unix socket."""
    if os.path.exists(path):
        os.unlink(path)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(path)
    server.listen(8)

    def handle(conn):
        with conn, conn.makefile("r") as lines:
            for line in lines:
                parts = line.split()
                if len(parts) == 3 and parts[0] == "add":
                    hub.add(parts[1], parts[2])
                elif len(parts) == 3 and parts[0] == "delay":
                    try:
                        hub.set_delay(parts[1], parts[2])
                    except ValueError:
                        pass
                elif len(parts) == 3 and parts[0] == "volume":
                    try:
                        hub.set_volume(parts[1], parts[2])
                    except ValueError:
                        pass
                elif len(parts) == 2 and parts[0] == "remove":
                    hub.remove(parts[1])

    def accept():
        while True:
            conn, _ = server.accept()
            threading.Thread(target=handle, args=(conn,), daemon=True).start()

    threading.Thread(target=accept, daemon=True).start()


def cmd_stream(args):
    control = None
    if len(args) >= 2 and args[0] == "--control":
        control, args = args[1], args[2:]

    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    eof = asyncio.Event()
    source = queue.Queue()
    hub = Hub(loop)
    threading.Thread(
        target=read_stdin, args=(source, lambda: loop.call_soon_threadsafe(eof.set)), daemon=True
    ).start()
    threading.Thread(target=pace, args=(source, hub.current_outputs), daemon=True).start()
    if control:
        serve_control(control, hub)

    # The receivers given at launch start together, as before.
    targets = [arg.partition("=") for arg in args]
    gate = StartGate(len(targets))
    loop.create_task(open_after(gate, START_TIMEOUT))
    for identifier, _, address in targets:
        hub.add(identifier, address, gate)

    loop.run_until_complete(eof.wait())
    emit("done")


def local_addresses():
    """Addresses of this Mac, so that it is not offered as its own receiver."""
    import ifaddr

    return {str(ip.ip) for adapter in ifaddr.get_adapters() for ip in adapter.ips}


async def scan_receivers():
    loop = asyncio.get_running_loop()
    found = await pyatv.scan(loop, timeout=SCAN_TIMEOUT)
    own = local_addresses()
    receivers, seen = [], set()
    for conf in found:
        protocols = {service.protocol for service in conf.services}
        if not (Protocol.AirPlay in protocols or Protocol.RAOP in protocols):
            continue
        if str(conf.address) in own or conf.identifier in seen:
            continue
        seen.add(conf.identifier)
        receivers.append({"identifier": conf.identifier, "name": conf.name, "address": str(conf.address)})
    return receivers


def cmd_scan():
    print(json.dumps(asyncio.run(scan_receivers())), flush=True)


# Calibration: an irregular click track, so each click can be matched to its place in the track.
CLICK_SEED = 7
CLICK_TRACK_SECONDS = 40
CLICK_RATE = 48000
CLICK_LENGTH = 0.005
CLICK_FREQUENCY = 1000.0


def click_times():
    """The start of every click in the calibration track, in seconds. The same every time."""
    import random

    rng = random.Random(CLICK_SEED)
    times, t = [0.5], 0.5
    while True:
        t += rng.uniform(0.3, 0.8)
        if t > CLICK_TRACK_SECONDS - 1:
            return times
        times.append(t)


def write_click_track(path):
    """Write the calibration track as a 16-bit mono WAV."""
    import math

    samples = bytearray(CLICK_RATE * CLICK_TRACK_SECONDS * 2)
    n_click = int(CLICK_LENGTH * CLICK_RATE)
    for t in click_times():
        start = int(t * CLICK_RATE)
        for i in range(n_click):
            phase = i / CLICK_RATE
            value = 0.8 * math.sin(2 * math.pi * CLICK_FREQUENCY * phase) * (1 - phase / CLICK_LENGTH)
            struct.pack_into("<h", samples, 2 * (start + i), int(value * 32767))
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(CLICK_RATE)
        w.writeframes(bytes(samples))


def click_onsets(path):
    """The times, in seconds, where clear clicks start in a recording."""
    import bisect  # noqa: F401  (used by callers)

    with wave.open(path, "rb") as w:
        rate = w.getframerate()
        data = w.readframes(w.getnframes())
    samples = array.array("h")
    samples.frombytes(data[: len(data) // 2 * 2])
    if sys.byteorder == "big":
        samples.byteswap()
    # Peak level in each millisecond, then a threshold well above the room's noise.
    step = max(1, rate // 1000)
    peaks = [max(abs(x) for x in samples[i:i + step]) for i in range(0, len(samples), step)]
    ordered = sorted(peaks)
    noise = ordered[len(ordered) // 2] if ordered else 0
    threshold = max(60, 5 * noise)
    onsets, last = [], -1.0
    above = False
    for i, value in enumerate(peaks):
        if value > threshold and not above:
            t = i / 1000
            if t - last > 0.02:
                onsets.append(t)
                last = t
            above = True
        elif value <= threshold:
            above = False
    return onsets


def find_lag(recording, clicks):
    """How late the clicks in `recording` are, against the calibration track, in milliseconds."""
    import bisect

    onsets = click_onsets(recording)
    if not onsets:
        raise RuntimeError("no clicks heard: turn the receiver up, or move it closer to the Mac")
    expected = click_times()
    best_lag, best_count = 0.0, 0
    # Try every whole millisecond of lag from 0 to 10 seconds.
    for lag_ms in range(0, 10001):
        lag = lag_ms / 1000
        count = 0
        for t in expected:
            i = bisect.bisect_left(onsets, t + lag - 0.01)
            if i < len(onsets) and onsets[i] <= t + lag + 0.01:
                count += 1
        if count > best_count:
            best_lag, best_count = lag_ms, count
    if best_count < max(3, len(expected) // 4):
        raise RuntimeError("the clicks were too faint to measure: turn the receiver up, or move it closer")
    # Refine: the median offset of the clicks that matched at that lag.
    offsets = []
    for t in expected:
        i = bisect.bisect_left(onsets, t + best_lag / 1000 - 0.01)
        if i < len(onsets) and onsets[i] <= t + best_lag / 1000 + 0.01:
            offsets.append(onsets[i] - t)
    offsets.sort()
    return {"lag_ms": round(offsets[len(offsets) // 2] * 1000, 1), "matched": best_count, "clicks": len(expected)}


def main(argv):
    if len(argv) >= 3 and argv[1] == "clicks":
        write_click_track(argv[2])
        return 0
    if len(argv) >= 4 and argv[1] == "lag":
        try:
            print(json.dumps(find_lag(argv[2], argv[3])), flush=True)
            return 0
        except RuntimeError as error:
            print(json.dumps({"error": str(error)}), flush=True)
            return 1
    if len(argv) >= 2 and argv[1] == "scan":
        cmd_scan()
    elif len(argv) >= 3 and argv[1] == "stream":
        cmd_stream(argv[2:])
    else:
        print(__doc__, file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
