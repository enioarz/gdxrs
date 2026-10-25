"""Fail-closed qualification/report tests; no large files or native build."""
import copy
from pathlib import Path
import sys
import unittest
sys.path.insert(0, str(Path(__file__).parents[1]))
import scale


class ScaleValidation(unittest.TestCase):
    def test_digest_detects_each_field_and_order(self):
        first = scale.expected(1)['all']
        second = scale.expected(2)['all']
        self.assertNotEqual(first['digest'], second['digest'])
        h = scale.INITIAL
        words = [1]*20 + [0x3ff0000000000000 | (f*12345+scale.SEED) for f in range(5)]
        for value in words:
            h = scale.step(h, value)
        self.assertEqual(first, dict(count=1, digest=f'{h:016x}'))
        for field in range(25):
            changed = words.copy()
            changed[field] ^= 1
            altered = scale.INITIAL
            for value in changed:
                altered = scale.step(altered, value)
            self.assertNotEqual(altered, h)
        self.assertNotEqual(scale.step(scale.step(h, 1), 2), scale.step(scale.step(h, 2), 1))

    def test_small_files_never_meet_scale_gate(self):
        for kind in ['records', 'metadata']:
            with self.assertRaisesRegex(ValueError, 'four times'):
                scale.check_scale(dict(bytes=scale.CAP+1, logical_output_bytes=scale.CAP+1, labels=100), kind)
        with self.assertRaisesRegex(ValueError, 'independently oversized'):
            scale.check_scale(dict(bytes=4*scale.CAP, logical_output_bytes=4*scale.CAP, labels=100), 'metadata')

    def test_full_digest_and_decoded_work_mandatory(self):
        recipe = dict(count=16, expected=scale.expected(16))
        trial = dict(result=dict(**recipe['expected']['all'], decoded_rows=16, first_bytes=1000, root_peak=1000))
        scale.validate_scan(trial, recipe, 'all')
        for field, value in [('count', 15), ('digest', '0'*16), ('decoded_rows', 15), ('first_bytes', 1024**2), ('root_peak', scale.BUDGET+1)]:
            invalid = copy.deepcopy(trial)
            invalid['result'][field] = value
            with self.assertRaises(ValueError):
                scale.validate_scan(invalid, recipe, 'all')

    def test_empty_and_exact_limits_are_distinct_from_full_decode(self):
        recipe = dict(count=16, expected=scale.expected(16))
        trial = dict(result=dict(count=0, digest=f'{scale.INITIAL:016x}', decoded_rows=0, root_peak=1000, first_bytes=0))
        scale.validate_scan(trial, recipe, 'zero')
        trial['result']['decoded_rows'] = 1
        with self.assertRaises(ValueError):
            scale.validate_scan(trial, recipe, 'zero')
        trial = dict(result=dict(**recipe['expected']['head'], decoded_rows=13, root_peak=1000, first_bytes=1000))
        scale.validate_scan(trial, recipe, 'head')

    def test_dispersion_cannot_be_claimed_from_one_trial(self):
        with self.assertRaises(ValueError):
            scale.summary([1.])
        result = scale.summary([1., 2., 3.])
        self.assertEqual(result['median'], 2.)
        self.assertEqual(result['trials'], 3)

    def test_label_and_request_recipe_preserves_boundaries_duplicates_unknown(self):
        self.assertEqual(len(scale.label(1)), 63)
        self.assertLess(scale.label(1), scale.label(36_000_000))
        self.assertEqual(scale.expected_lookup(64)['count'], 18)
        self.assertEqual(scale.expected_lookup(4)['count'], 2)
        self.assertNotEqual(scale.expected_labels(3), scale.expected_labels(4))
