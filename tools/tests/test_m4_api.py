"""Current API audit must retain the frozen required roster and actual methods."""
import copy
from pathlib import Path
import sys
import unittest
sys.path.insert(0, str(Path(__file__).parents[1]))
import m4_api as m

class M4Api(unittest.TestCase):
    def test_missing_duplicate_unknown_pending_and_weakened_requirements_fail(self):
        rows = m.read(m.ROOT/'docs/m4-api.csv')
        roster = [r['native_function'] for r in rows]
        m.validate(rows, roster)
        variants = [rows[:-1], rows+[rows[0]]]
        for field, value in [('native_function', 'notNative'), ('disposition', 'pending'),
                             ('required_for_m4', 'no'), ('disposition', 'excluded'),
                             ('rust_methods', 'nonexistent_method'), ('evidence', 'AGENTS.md')]:
            bad = copy.deepcopy(rows)
            target = next(r for r in bad if r['required_for_m4'] == 'yes')
            target[field] = value
            variants.append(bad)
        for bad in variants:
            with self.assertRaises(ValueError):
                m.validate(bad, roster)
