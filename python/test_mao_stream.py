import queue
import struct
import unittest

import mao_stream as m


class TakeTickTests(unittest.TestCase):
    def test_silence_while_tick_is_incomplete_and_bytes_are_kept(self):
        pending = bytearray(b"\x01" * (m.TICK_BYTES - 4))
        self.assertEqual(m.take_tick(pending), m.SILENCE)
        self.assertEqual(len(pending), m.TICK_BYTES - 4)

    def test_returns_one_tick_when_enough_audio_is_pending(self):
        pending = bytearray(b"\x07" * (m.TICK_BYTES + 8))
        chunk = m.take_tick(pending)
        self.assertEqual(chunk, b"\x07" * m.TICK_BYTES)
        self.assertEqual(len(pending), 8)

    def test_backlog_is_capped_and_stays_frame_aligned(self):
        pending = bytearray(b"\x00" * (m.MAX_PENDING_BYTES * 3))
        m.take_tick(pending)
        self.assertLessEqual(len(pending), m.MAX_PENDING_BYTES + m.TICK_BYTES)
        self.assertEqual(len(pending) % m.BYTES_PER_FRAME, 0)

    def test_silence_tick_is_the_right_length(self):
        self.assertEqual(len(m.SILENCE), m.TICK_BYTES)


class HeaderTests(unittest.TestCase):
    def test_header_describes_48k_stereo_16bit(self):
        header = m.wav_header()
        self.assertEqual(header[:4], b"RIFF")
        self.assertEqual(header[8:12], b"WAVE")
        channels, rate = struct.unpack("<HI", header[22:28])
        bits = struct.unpack("<H", header[34:36])[0]
        self.assertEqual((channels, rate, bits), (2, 48000, 16))


class DiscoveryTests(unittest.TestCase):
    def test_this_mac_has_local_addresses(self):
        self.assertTrue(m.local_addresses())


class PaceTests(unittest.TestCase):
    def test_pace_ends_every_output_with_none(self):
        source = queue.Queue()
        source.put(b"\x00" * m.TICK_BYTES)
        source.put(None)
        outputs = [queue.Queue(), queue.Queue()]
        m.pace(source, outputs)
        for out in outputs:
            items = []
            while not out.empty():
                items.append(out.get_nowait())
            self.assertIsNone(items[-1])
            self.assertTrue(all(len(i) == m.TICK_BYTES for i in items[:-1]))


if __name__ == "__main__":
    unittest.main()
