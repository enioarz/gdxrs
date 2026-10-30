"""M4 must never qualify smoke, missing families or undersized diagnostics."""
import copy
from pathlib import Path
import sys
import unittest
sys.path.insert(0, str(Path(__file__).parents[1]))
import m4_scale as m

class M4Scale(unittest.TestCase):
    def cases(self):
        return [dict(kind=k, count=n, bytes=4*m.CAP+1, logical_bytes=4*m.CAP+1,
                     labels=(4*m.CAP)//64+1,
                     worker=dict(peak_rss_bytes=10, result=dict(error_bytes=4*m.CAP+1)))
                for k in m.KINDS for n in [1, 4]]
    def test_missing_family_growth_physical_logical_and_diagnostics_are_rejected(self):
        cases = self.cases()
        m.qualify(cases)
        with self.assertRaises(ValueError):
            m.qualify(cases[:-2])
        with self.assertRaises(ValueError):
            m.qualify(cases[::2])
        for field in ['bytes', 'logical_bytes', 'labels']:
            bad = copy.deepcopy(cases)
            next(c for c in bad if c['kind'] == 'dictionary' and c['count'] == 4)[field] = 4
            with self.assertRaises(ValueError):
                m.qualify(bad)
        bad = copy.deepcopy(cases)
        next(c for c in bad if c['kind'] == 'errors' and c['count'] == 4)['worker']['result']['error_bytes'] = 4*m.CAP
        with self.assertRaises(ValueError):
            m.qualify(bad)
    def test_shared_root_and_both_os_limits_are_required(self):
        want = dict(count=8, labels=65536, errors=0, error_bytes=0, error_digest='cbf29ce484222325')
        good = dict(result=dict(want, os_cap_verified=True, root_limit=m.BUDGET, root_peak=100,
                               shared_root=True, scratch_peak=0, scratch_peak_handles=0,
                               **dict.fromkeys(m.PHASES, 0)),
                    os_cap=dict(hard_bytes=m.CAP, soft_bytes=m.CAP), peak_rss_bytes=100)
        m.validate_worker(good, want, 'append')
        for scope, field, value in [('result', 'shared_root', False), ('result', 'root_peak', m.BUDGET+1),
                                    ('os_cap', 'hard_bytes', m.CAP*2), ('os_cap', 'soft_bytes', m.CAP*2),
                                    ('result', 'scratch_peak_handles', 15)]:
            bad = copy.deepcopy(good)
            bad[scope][field] = value
            with self.assertRaises(ValueError):
                m.validate_worker(bad, want, 'append')
    def test_small_python_recipe_uses_original_ordinal_order_and_exact_bits(self):
        want = m.python_expected('errors', 8)
        self.assertEqual(want['errors'], 7)
        self.assertEqual(want['error_bytes'], 1113)
        self.assertEqual(m.python_expected('dictionary', 8)['count'], 5)
        with self.assertRaises(ValueError):
            m.python_expected('errors', 1_000_000)
