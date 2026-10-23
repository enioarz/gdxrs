"""Real corpus tampering checks, isolated from tracked inputs."""

import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parents[1]))
import dev
import fixtures


class FixtureChecks(unittest.TestCase):
    def copy(self, directory):
        root = Path(directory)/'fixtures'
        shutil.copytree(fixtures.FIXTURES,root)
        return root

    def test_changed_expected_record_even_with_updated_hash_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = self.copy(directory)
            manifest = json.loads((root/'manifest.json').read_text())
            item = manifest['fixtures'][0]
            path = root/item['expectation']
            text = path.read_text()
            old = 'R\t1\t1\tf:3ff0000000000000'
            self.assertIn(old,text)
            path.write_text(text.replace(old,'R\t1\t1\tf:4000000000000000',1))
            item['expectation_sha256'] = fixtures.digest(path)
            fixtures.save_json(root/'manifest.json',manifest)
            with self.assertRaisesRegex(ValueError,'expectation mismatch'):
                fixtures.check(dev.run,root)

    def test_missing_required_file_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = self.copy(directory)
            (root/'v7/semantics-0.gdx').unlink()
            with self.assertRaisesRegex(ValueError,'missing/outside'):
                fixtures.check(dev.run,root)

    def test_altered_artifact_byte_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = self.copy(directory)
            path = root/'v7/semantics-0.gdx'
            data = bytearray(path.read_bytes()); data[-1] ^= 1; path.write_bytes(data)
            with self.assertRaisesRegex(ValueError,'digest mismatch'):
                fixtures.check(dev.run,root)

    def test_empty_duplicate_and_path_escape_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = self.copy(directory)
            original = json.loads((root/'manifest.json').read_text())
            for bad in ([],[original['fixtures'][0]]*len(original['fixtures'])):
                manifest = dict(original,fixtures=bad)
                fixtures.save_json(root/'manifest.json',manifest)
                with self.assertRaisesRegex(ValueError,'mandatory fixture IDs'):
                    fixtures.check(dev.run,root)
            with self.assertRaisesRegex(ValueError,'missing/outside'):
                fixtures.checked_path(root,'../outside')


if __name__ == '__main__':
    unittest.main()
