"""Prevent stale or substituted native artifacts from satisfying an oracle check."""

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("oracle", Path(__file__).parents[1] / "oracle.py")
oracle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(oracle)


class Identity(unittest.TestCase):
    def test_missing_changed_binary_and_changed_source_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            area = Path(directory)
            binary = area / "driver"
            binary.write_bytes(b"original")
            metadata = {"source_fingerprint": "source-a", "executable": str(binary),
                        "executable_sha256": hashlib.sha256(b"original").hexdigest()}
            (area / "current.json").write_text(json.dumps(metadata))
            with patch.object(oracle, "AREA", area), patch.object(oracle, "source_fingerprint", return_value="source-a"):
                self.assertEqual(oracle.executable(), binary)
                binary.write_bytes(b"different")
                with self.assertRaisesRegex(ValueError, "executable changed"):
                    oracle.executable()
                binary.unlink()
                with self.assertRaisesRegex(ValueError, "invalid oracle path"):
                    oracle.executable()
            with patch.object(oracle, "AREA", area), patch.object(oracle, "source_fingerprint", return_value="source-b"):
                with self.assertRaisesRegex(ValueError, "sources changed"):
                    oracle.executable()


if __name__ == "__main__":
    unittest.main()
