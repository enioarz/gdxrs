"""Strict independent checks for bounded writer development probes."""
import sys
from pathlib import Path
import tempfile
import unittest
import zlib

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import writer


class BlockProbe(unittest.TestCase):
    def test_independent_stored_zlib_and_empty_vectors(self):
        self.assertEqual(writer.decode_blocks(b''),(b'',[]))
        self.assertEqual(writer.decode_blocks(b'\x00\x00\x01x'),(b'x',[dict(tag=0,encoded=1,decoded=1)]))
        payload = zlib.compress(b'x'*32768)
        decoded,blocks = writer.decode_blocks(b'\x01'+len(payload).to_bytes(2,'big')+payload)
        self.assertEqual(decoded,b'x'*32768)
        self.assertEqual(blocks,[dict(tag=1,encoded=len(payload),decoded=32768)])

    def test_rejects_bad_framing_checksums_expansion_and_trailing_data(self):
        payload = zlib.compress(b'x'*32768)
        variants = [b'', b'\x02\x00\x01x', b'\x00\x00\x00', b'\x00\x00\x02x', b'\x01\x00',
                    b'\x00\x80\x01'+b'x'*32769]
        # The empty stream itself is valid; a truncated nonempty header is not.
        variants[0] = b'\x00'
        for body in [payload[:-1],payload[:-1]+bytes([payload[-1]^1]),payload+b'x',zlib.compress(b'x'*32769)]:
            variants.append(b'\x01'+len(body).to_bytes(2,'big')+body)
        for data in variants:
            with self.subTest(size=len(data)):
                with self.assertRaises((ValueError,zlib.error)):
                    writer.decode_blocks(data)
        with self.assertRaises(ValueError):
            writer.decode_blocks((b'\x00\x80\x00'+b'x'*32768)*5)

    def test_probe_file_collection_has_explicit_limit(self):
        with tempfile.TemporaryDirectory() as area:
            path=Path(area)/'probe'
            path.write_bytes(b'x'*131072)
            self.assertEqual(len(writer.bounded_read(path)),131072)
            path.write_bytes(b'x'*131073)
            with self.assertRaises(ValueError): writer.bounded_read(path)
