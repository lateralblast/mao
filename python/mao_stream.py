#!/usr/bin/env python3
"""mao-stream: send system audio to AirPlay receivers.

    mao_stream.py scan
        Print the AirPlay receivers on the local network as a JSON list.

    mao_stream.py devices
        Print this Mac's output devices as a JSON list.

    mao_stream.py stream TARGET [TARGET ...]
        Read 48 kHz, 16-bit, stereo PCM from stdin and send it to each target.
        A target is IDENTIFIER=ADDRESS for an AirPlay receiver, or local:NAME
        for a device on this Mac. Exits when stdin closes.

Progress goes to stdout as one JSON object per line, for example:
    {"event": "receiver", "identifier": "...", "state": "streaming", "message": ""}
"""

import asyncio
import io
import json
import os
import queue
import struct
import sys
import threading
import time

import pyatv
from pyatv.const import Protocol

RATE = 48000
CHANNELS = 2
BYTES_PER_FRAME = CHANNELS * 2
TICK_SECONDS = 0.02
TICK_BYTES = int(RATE * BYTES_PER_FRAME * TICK_SECONDS)  # 20 ms = 3840 bytes
MAX_PENDING_BYTES = RATE * BYTES_PER_FRAME // 4  # keep at most 250 ms of backlog
RECEIVER_QUEUE_TICKS = 250  # about 5 s of audio per receiver before dropping
SCAN_TIMEOUT = 6
HOST_TIMEOUT = 3
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


def read_stdin(source):
    while True:
        data = os.read(0, 65536)
        if not data:
            source.put(None)
            # The caller has gone. Do not wait on receivers that may never finish.
            threading.Timer(EXIT_GRACE_SECONDS, os._exit, [0]).start()
            return
        source.put(data)


def pace(source, outputs):
    """Feed every receiver one tick of audio every 20 ms, in real time."""
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
        for out in outputs:
            try:
                out.put_nowait(chunk)
            except queue.Full:
                pass  # this receiver is behind: drop audio rather than stall the others
        deadline += TICK_SECONDS
        delay = deadline - time.monotonic()
        if delay > 0:
            time.sleep(delay)
        else:
            deadline = time.monotonic()  # fell behind: resynchronise the clock
    for out in outputs:
        out.put(None)


def write_receiver(out, fd, ready):
    """Write the header, then forward audio once the receiver is connected.

    Audio queued before the connection is ready is discarded, so the receiver
    starts with what is playing now rather than what was queued earlier."""
    try:
        write_all(fd, wav_header())
        while True:
            chunk = out.get()
            if chunk is None:
                break
            if ready.is_set():
                write_all(fd, chunk)
    except OSError:
        pass  # the receiver has disconnected
    finally:
        os.close(fd)


LOCAL_PREFIX = "local:"


def local_devices():
    """This Mac's output devices that can take a copy of the audio."""
    import sounddevice as sd

    devices, seen = [], set()
    for info in sd.query_devices():
        name = info["name"]
        if info["max_output_channels"] < CHANNELS or name in seen:
            continue
        seen.add(name)
        devices.append({"identifier": LOCAL_PREFIX + name, "name": name, "address": ""})
    return devices


def play_local(out, name, ready):
    """Play the copied audio through one of this Mac's output devices.

    The device's own clock paces the writes, so this thread blocks in the
    stream's write call and the queue absorbs any short stalls."""
    import sounddevice as sd

    identifier = LOCAL_PREFIX + name
    try:
        stream = sd.RawOutputStream(
            device=name, samplerate=RATE, channels=CHANNELS, dtype="int16", latency="low"
        )
        stream.start()
    except Exception as error:
        emit("receiver", identifier=identifier, state="error", message=describe(error))
        while out.get() is not None:
            pass
        return

    ready.set()
    emit("receiver", identifier=identifier, state="streaming", message="")
    try:
        while True:
            chunk = out.get()
            if chunk is None:
                break
            stream.write(chunk)
        emit("receiver", identifier=identifier, state="ended", message="")
    except Exception as error:
        emit("receiver", identifier=identifier, state="error", message=describe(error))
    finally:
        stream.stop()
        stream.close()


async def stream_receiver(loop, identifier, address, reader, ready):
    emit("receiver", identifier=identifier, state="connecting", message="")
    try:
        found = await pyatv.scan(loop, timeout=HOST_TIMEOUT, hosts=[address])
        conf = next((c for c in found if c.identifier == identifier), None)
        if conf is None:
            raise RuntimeError(f"no receiver with identifier {identifier} at {address}")
        atv = await pyatv.connect(conf, loop, protocol=Protocol.AirPlay)
    except Exception as error:
        emit("receiver", identifier=identifier, state="error", message=describe(error))
        return

    ready.set()
    emit("receiver", identifier=identifier, state="streaming", message="")
    try:
        await atv.stream.stream_file(reader)
        emit("receiver", identifier=identifier, state="ended", message="")
    except Exception as error:
        emit("receiver", identifier=identifier, state="error", message=describe(error))
    finally:
        atv.close()


async def stream_all(specs):
    loop = asyncio.get_running_loop()
    await asyncio.gather(
        *(stream_receiver(loop, ident, addr, reader, ready) for ident, addr, reader, ready in specs)
    )


def cmd_stream(args):
    source = queue.Queue()
    threading.Thread(target=read_stdin, args=(source,), daemon=True).start()

    outputs, airplay, local = [], [], []
    for arg in args:
        out = queue.Queue(maxsize=RECEIVER_QUEUE_TICKS)
        outputs.append(out)
        if arg.startswith(LOCAL_PREFIX):
            ready = threading.Event()
            threading.Thread(target=play_local, args=(out, arg[len(LOCAL_PREFIX):], ready), daemon=True).start()
            continue
        identifier, _, address = arg.partition("=")
        read_fd, write_fd = os.pipe()
        reader = io.BufferedReader(io.FileIO(read_fd, "rb"))
        ready = threading.Event()
        threading.Thread(target=write_receiver, args=(out, write_fd, ready), daemon=True).start()
        airplay.append((identifier, address, reader, ready))

    threading.Thread(target=pace, args=(source, outputs), daemon=True).start()
    if airplay:
        asyncio.run(stream_all(airplay))
    else:
        # Only local outputs: nothing to wait for but the pacer, which ends at stdin EOF.
        threading.Event().wait()
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


def main(argv):
    if len(argv) >= 2 and argv[1] == "scan":
        cmd_scan()
    elif len(argv) >= 2 and argv[1] == "devices":
        print(json.dumps(local_devices()), flush=True)
    elif len(argv) >= 3 and argv[1] == "stream":
        cmd_stream(argv[2:])
    else:
        print(__doc__, file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
