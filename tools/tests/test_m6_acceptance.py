import copy
import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parents[1]))
import m6_acceptance as a


def trial():
    return dict(exit_code=0, os_cap=dict(soft_bytes=a.m5.CAP, hard_bytes=a.m5.CAP),
                peak_rss_bytes=10000, sampled_peak_fds=4,
                result=dict(root_limit=a.m5.BUDGET, root_peak=1000, os_cap_verified=True,
                            vm_peak_bytes=100000))


def serial_result(count=4096):
    return dict(count=count, digest='aa', seconds=12.0, open_seconds=0.1, setup_seconds=0.1,
                first_seconds=1.0, batch_seconds=8.0, sink_seconds=1.8, close_seconds=0.0,
                batches=4, decoded_rows=count, returned_rows=count, root_limit=a.m5.BUDGET,
                root_peak=1000, os_cap_verified=True, vm_peak_bytes=100000)


def phase(workers=2, query='all', count=4096):
    admitted = 0 if query == 'zero' else workers
    return dict(query=query, workers=workers, count=count, digest='aa', seconds=5.0, first_seconds=1.0,
                batch_seconds=3.0, sink_seconds=1.0, requested=workers, admitted=admitted,
                joined=admitted, maximum_buffered_batches=admitted, peak_ready_batches=admitted,
                stack_reserved=admitted * (2 * 1024**2 + 64 * 1024),
                control_reserved=admitted * 256 * 1024, tasks_dispatched=admitted,
                tasks_completed=admitted, decoded_rows=count, produced_rows=count,
                returned_rows=count, complete=query != 'cancel', cancelled=query == 'cancel')


def parallel_result(phases):
    return dict(mode='parallel', count=4096, reopen_seconds=2.0, phases=phases,
                root_limit=a.m5.BUDGET, root_peak=1000, os_cap_verified=True, vm_peak_bytes=100000)


def m5_phase(query='all', count=4096, digest='aa', seconds=5.0):
    return dict(query=query, count=count, digest=digest, seconds=seconds, first_seconds=1.0,
                decoded_rows=count, returned_rows=count)


def build_result(count=4096):
    return dict(result=dict(root_limit=a.m5.BUDGET, root_peak=1000, os_cap_verified=True,
                            index_quota=a.m5.QUOTA, handle_limit=2, cache_limit=1024,
                            phases=[dict(seconds=20.0, source_bytes_hashed=582094, rows_decoded=count,
                                         intervals=1, directory_bytes=1024, index_bytes=4096,
                                         pages_read=4, pages_written=4, digest_seconds=1.0,
                                         replay_seconds=1.0, complete=True, published=True)]),
                exit_code=0, os_cap=dict(soft_bytes=a.m5.CAP, hard_bytes=a.m5.CAP),
                peak_rss_bytes=10000, sampled_peak_fds=3)


def session_result(count=4096, source_bytes=582094):
    return dict(result=dict(root_limit=a.m5.BUDGET, root_peak=1000, os_cap_verified=True,
                            index_quota=a.m5.QUOTA, handle_limit=2, cache_limit=1024,
                            phases=[dict(seconds=2.0, source_bytes_hashed=source_bytes, rows_decoded=count,
                                         intervals=1, directory_bytes=1024, index_bytes=4096,
                                         pages_read=4, pages_written=0, digest_seconds=1.0,
                                         replay_seconds=1.0, complete=True, published=False),
                                    m5_phase('all', count)]),
                exit_code=0, os_cap=dict(soft_bytes=a.m5.CAP, hard_bytes=a.m5.CAP),
                peak_rss_bytes=10000, sampled_peak_fds=3)


def case(kind='records', n=4096):
    def ph(workers, query, count):
        p = phase(workers, query, count)
        p['digest'] = a.expected_prefix(kind, count)['digest'] if count else f'{a.scale.INITIAL:016x}'
        if query == 'all':
            p['digest'] = 'aa'
        return p
    t = dict(serial=dict(trial(), result=serial_result(n)),
             build=build_result(n), session=session_result(n),
             **{f'parallel-{w}': dict(trial(), result=parallel_result([ph(w, 'all', n)])) for w in a.WORKERS},
             **{'parallel-cancel': dict(trial(), result=parallel_result([ph(4, 'cancel', 100)])),
                'parallel-head': dict(trial(), result=parallel_result([ph(2, 'head', 13)])),
                'parallel-zero': dict(trial(), result=parallel_result([ph(2, 'zero', 0)])),
                'parallel-warm': dict(trial(), result=parallel_result([ph(4, 'all', n)] * 3))})
    return dict(kind=kind, count=n, source=dict(bytes=582094, sha256='ab'),
                logical_bytes=n * 120, expected_all=dict(count=n, digest='aa'), trials=[t, t, t])


def oversized(kind, n):
    c = case(kind, n)
    c['source']['bytes'] = 5 * a.m5.CAP
    c['logical_bytes'] = 5 * a.m5.CAP
    if kind == 'metadata':
        c['logical_bytes'] = n * 64
    for t in c['trials']:
        t['session']['result']['phases'][0]['source_bytes_hashed'] = c['source']['bytes']
    return c


class AcceptanceGuards(unittest.TestCase):
    def test_valid_case_passes_scale_guards(self):
        cases = [oversized('records', 4096), oversized('records', 16384),
                 oversized('compressed', 4096), oversized('compressed', 16384),
                 oversized('metadata', 33_600_000), oversized('metadata', 67_200_000)]
        a.qualify(cases)

    def test_containment_is_required(self):
        for change in [lambda t: t['result'].update(root_peak=a.m5.BUDGET + 1),
                       lambda t: t['result'].update(os_cap_verified=False),
                       lambda t: t['result'].update(vm_peak_bytes=a.m5.CAP + 1),
                       lambda t: t.update(peak_rss_bytes=a.m5.CAP + 1),
                       lambda t: t.update(sampled_peak_fds=7)]:
            bad = trial()
            change(bad)
            with self.assertRaises(ValueError):
                a.validate_containment(bad)

    def test_serial_phase_accounting_is_required(self):
        for change in [lambda r: r.update(digest='ff'), lambda r: r.update(seconds=-1.0),
                       lambda r: r.update(batch_seconds=100.0), lambda r: r.update(batches=0),
                       lambda r: r.update(decoded_rows=1)]:
            r = serial_result()
            change(r)
            t = trial()
            t['result'] = r
            with self.assertRaises(ValueError):
                a.validate_serial(t, dict(count=4096, digest='aa'))

    def test_parallel_admission_queue_and_cancel_are_required(self):
        base = phase()
        mutations = [lambda p: p.update(admitted=1), lambda p: p.update(joined=1),
                     lambda p: p.update(peak_ready_batches=5), lambda p: p.update(stack_reserved=0),
                     lambda p: p.update(control_reserved=0), lambda p: p.update(cancelled=True),
                     lambda p: p.update(seconds=0.0)]
        for change in mutations:
            p = copy.deepcopy(base)
            change(p)
            with self.assertRaises(ValueError):
                a.validate_phase(p, 2, 'all', dict(count=4096, digest='aa'), 'records')
        for query, count in [('zero', 4096), ('cancel', 0), ('cancel', 5000)]:
            p = copy.deepcopy(base)
            p.update(query=query, count=count)
            with self.assertRaises(ValueError):
                a.validate_phase(p, 2, query, dict(count=count, digest='aa'), 'records')

    def test_qualify_rejects_missing_oversized_or_trials(self):
        good = [oversized('records', 4096), oversized('records', 16384),
                oversized('compressed', 4096), oversized('compressed', 16384),
                oversized('metadata', 33_600_000), oversized('metadata', 67_200_000)]
        for mutate in [lambda cs: cs.pop(), lambda cs: cs[0]['trials'].pop(),
                       lambda cs: cs[1].update(count=1),
                       lambda cs: cs[1]['source'].update(bytes=1000),
                       lambda cs: cs[3].update(logical_bytes=1000),
                       lambda cs: cs[5].update(logical_bytes=1000),
                       lambda cs: cs[1]['trials'][0]['serial'].update(peak_rss_bytes=a.m5.CAP + 1)]:
            bad = copy.deepcopy(good)
            mutate(bad)
            with self.assertRaises(ValueError):
                a.qualify(bad)

    def test_metadata_prefix_matches_step_recipe(self):
        p = a.expected_metadata_prefix(3)
        h = a.scale.INITIAL
        for n in (1, 2, 3):
            h = a.scale.step(a.scale.step(h, n), 0)
        self.assertEqual(p, dict(count=3, digest=f'{h:016x}'))
