import copy
import sys
import tempfile
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parents[1]))
import m6_policy as p


def trial(count=4096):
    return dict(exit_code=0, os_cap=dict(soft_bytes=p.m5.CAP, hard_bytes=p.m5.CAP),
                peak_rss_bytes=10000, sampled_peak_fds=4,
                result=dict(root_limit=p.m5.BUDGET, root_peak=1000, os_cap_verified=True,
                            vm_peak_bytes=100000, count=count, serial_seconds=10.0, build_seconds=20.0,
                            reopen_seconds=2.0, indexed_seconds=1.0, leading_seconds=0.1,
                            leading_count=32, serial_digest='aa', leading_digest='bb', parallel_seconds=[4.0, 3.0, 2.0],
                            parallel_digest='aa',
                            shapes=[dict(shape='all', choice='indexed', workers=0, seconds=1.0, count=count, digest='aa'),
                                    dict(shape='leading', choice='indexed', workers=0, seconds=0.1, count=32, digest='bb'),
                                    dict(shape='head', choice='serial', workers=0, seconds=0.5, count=min(13, count), digest='cc'),
                                    dict(shape='zero', choice='serial', workers=0, seconds=0.0, count=0, digest='dd')]))


def case(kind, n):
    t = trial(n)
    return dict(kind=kind, count=n, result=t['result'], trials=[t])


class PolicyGuards(unittest.TestCase):
    def test_valid_case_passes(self):
        p.validate_case(case('records', 4096))

    def test_containment_is_required(self):
        for change in [lambda t: t['result'].update(root_peak=p.m5.BUDGET + 1),
                       lambda t: t['result'].update(os_cap_verified=False),
                       lambda t: t['result'].update(vm_peak_bytes=0),
                       lambda t: t['result'].update(vm_peak_bytes=p.m5.CAP + 1),
                       lambda t: t.update(peak_rss_bytes=p.m5.CAP + 1),
                       lambda t: t.update(sampled_peak_fds=7),
                       lambda t: t['os_cap'].update(hard_bytes=p.m5.CAP + 1)]:
            bad = trial()
            change(bad)
            with self.assertRaises(ValueError):
                p.validate_containment(bad)

    def test_digest_and_shape_contracts_are_required(self):
        mutations = [
            lambda c: c['result'].update(parallel_digest='ff'),
            lambda c: c['result']['shapes'][0].update(digest='ff'),
            lambda c: c['result']['shapes'][1].update(digest='ff'),
            lambda c: c['result']['shapes'][2].update(choice='parallel'),
            lambda c: c['result']['shapes'][3].update(choice='indexed'),
            lambda c: c['result']['shapes'][0].update(count=3),
            lambda c: c['result']['shapes'].pop(),
            lambda c: c['result']['shapes'][0].update(seconds=100.0),
            lambda c: c['result'].update(serial_seconds=-1.0),
        ]
        for change in mutations:
            bad = case('records', 4096)
            change(bad)
            with self.assertRaises(ValueError):
                p.validate_case(bad)

    def test_each_query_shape_and_worker_measurement_occurs_once(self):
        for change in [
            lambda c: c['result']['shapes'].append(copy.deepcopy(c['result']['shapes'][0])),
            lambda c: c['result']['parallel_seconds'].pop(),
            lambda c: c['result']['parallel_seconds'].append(1.0),
        ]:
            bad = case('records', 4096)
            change(bad)
            with self.assertRaises(ValueError):
                p.validate_case(bad)

    def test_planned_times_are_finite_and_nonnegative(self):
        for shape in range(4):
            for duration in [float('nan'), float('inf'), -1.0]:
                bad = case('records', 4096)
                bad['result']['shapes'][shape]['seconds'] = duration
                with self.assertRaises(ValueError):
                    p.validate_case(bad)
        bad = case('records', 4096)
        bad['result']['shapes'][0]['seconds'] = 0.0
        with self.assertRaises(ValueError):
            p.validate_case(bad)

    def test_inputs_are_retired_only_after_result_and_identity_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            source, index = Path(directory) / 'source.gdx', Path(directory) / 'index'
            source.write_bytes(b'owned source')
            index.write_bytes(b'owned index')
            good = case('records', 4096)
            good['source'] = p.writer_scale.hash_record(source)
            bad = copy.deepcopy(good)
            bad['result']['shapes'][0]['digest'] = 'wrong'
            with self.assertRaises(ValueError):
                p.validate_and_retire(bad, source, index)
            self.assertTrue(source.exists() and index.exists())
            source.write_bytes(b'changed source')
            with self.assertRaisesRegex(ValueError, 'mutated source'):
                p.validate_and_retire(good, source, index)
            self.assertTrue(source.exists() and index.exists())
            source.write_bytes(b'owned source')
            p.validate_and_retire(good, source, index)
            self.assertFalse(source.exists() or index.exists())

    def test_qualify_requires_growing_complete_families(self):
        cases = [case(k, n) for k in p.KINDS for n in [4096, 16384]]
        p.qualify(cases)
        for mutate in [lambda c: c.pop(), lambda c: c[-1].update(kind='records'),
                       lambda c: c[-1].update(count=1),
                       lambda c: c[0]['trials'][0].update(peak_rss_bytes=p.m5.CAP + 1)]:
            bad = copy.deepcopy(cases)
            mutate(bad)
            with self.assertRaises(ValueError):
                p.qualify(bad)
