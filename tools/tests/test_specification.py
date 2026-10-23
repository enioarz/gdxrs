"""Specification gates must fail when independent evidence/inventory is weakened."""

import csv
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0,str(Path(__file__).parents[1]))
import dev
import specification


class SpecificationChecks(unittest.TestCase):
    def copy(self,directory):
        docs = Path(directory)/'docs'
        shutil.copytree(specification.ROOT/'docs',docs)
        return docs

    def test_missing_public_function_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            docs = self.copy(directory)
            path = docs/'api-parity.csv'
            lines = path.read_text().splitlines()
            path.write_text('\n'.join(lines[:-1])+'\n')
            with self.assertRaisesRegex(ValueError,'API inventory'):
                specification.check(dev.run,docs=docs)

    def test_changed_annotated_record_bytes_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            docs = self.copy(directory)
            path = docs/'format-evidence.json'
            ledger = json.loads(path.read_text())
            rule = next(r for r in ledger['rules'] if r['id']=='F12')
            rule['expected_observations']['prefix_records'][0] = '00'
            path.write_text(json.dumps(ledger))
            with self.assertRaisesRegex(ValueError,'observation mismatch: F12'):
                specification.check(dev.run,docs=docs)

    def test_missing_property_accessor_and_unverified_rule_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            docs = self.copy(directory)
            path = docs/'api-properties.csv'
            lines = path.read_text().splitlines(); path.write_text('\n'.join(lines[:-1])+'\n')
            with self.assertRaisesRegex(ValueError,'API inventory'):
                specification.check(dev.run,docs=docs)
            ledger_path = docs/'format-evidence.json'
            ledger = json.loads(ledger_path.read_text());ledger['rules'][0]['status']='pending'
            ledger_path.write_text(json.dumps(ledger))
            with self.assertRaisesRegex(ValueError,'unverified rule'):
                specification.check(dev.run,docs=docs)


if __name__ == '__main__':
    unittest.main()
