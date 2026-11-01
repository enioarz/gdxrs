import copy
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))
import m6_profile as m


def scan():
    return dict(exit_code=0, os_cap=dict(soft_bytes=m.m5.CAP, hard_bytes=m.m5.CAP),
                peak_rss_bytes=10000, sampled_peak_fds=4,
                result=dict(count=100, digest='aabb', root_limit=m.m5.BUDGET, root_peak=1000,
                            os_cap_verified=True, seconds=1., first_seconds=.2, open_seconds=.1,
                            setup_seconds=.1, batch_seconds=.5, sink_seconds=.2, close_seconds=.05,
                            batches=1, decoded_rows=100, returned_rows=100))


def worker(phases):
    return dict(result=dict(root_limit=m.m5.BUDGET, root_peak=1000, os_cap_verified=True,
                            index_quota=m.m5.QUOTA, handle_limit=2, cache_limit=1024, phases=phases),
                os_cap=dict(soft_bytes=m.m5.CAP, hard_bytes=m.m5.CAP), peak_rss_bytes=10000,
                sampled_peak_fds=5)


def case(count=100):
    def query(name='leading', seconds=.1):
        return dict(query=name, seconds=seconds, count=count, digest='aabb', returned_rows=count,
                    decoded_rows=count, bytes_read=1000, decoded_buffers=2, index_pages_read=1,
                    validation_bytes_hashed=5000, validation_pages=20)
    opened = dict(complete=True, published=False, rows_decoded=count, source_bytes_hashed=5000,
                  pages_read=20, index_bytes=10000, seconds=2., digest_seconds=.5, replay_seconds=1.)
    profile=scan();profile['result'].update(count=count, decoded_rows=count, returned_rows=count)
    trial = dict(cold_serial=worker([query(seconds=1.)]),
                 build=worker([dict(opened, published=True)]),
                 session=worker([opened, query(), query(), query(), query(), query('all')]),
                 serial_matrix=worker([query('all')]), profile_scan=profile,
                 index=dict(bytes=10000), source_identity_rechecked=True)
    return dict(kind='records', count=count, source=dict(bytes=5000), expected_all=dict(count=count, digest='aabb'),
                trials=[copy.deepcopy(trial) for _ in range(3)])


class ProfileGuards(unittest.TestCase):
    def test_complete_exact_scan_is_mandatory(self):
        target=dict(count=100, digest='aabb')
        m.validate_scan(scan(), target)
        for field, value in [('digest', 'wrong'), ('count', 99), ('decoded_rows', 99),
                             ('returned_rows', 99), ('batches', 0)]:
            bad=scan();bad['result'][field]=value
            with self.assertRaises(ValueError):m.validate_scan(bad, target)

    def test_memory_and_actual_descriptors_cannot_be_omitted(self):
        for field, value in [('root_limit', m.m5.BUDGET+1), ('root_peak', m.m5.BUDGET+1),
                             ('os_cap_verified', False)]:
            bad=scan();bad['result'][field]=value
            with self.assertRaises(ValueError):m.validate_scan(bad, dict(count=100, digest='aabb'))
        for mutate in [lambda t:t['os_cap'].update(soft_bytes=m.m5.CAP+1),
                       lambda t:t['os_cap'].update(hard_bytes=m.m5.CAP+1),
                       lambda t:t.update(peak_rss_bytes=m.m5.CAP+1),
                       lambda t:t.update(sampled_peak_fds=5)]:
            bad=scan();mutate(bad)
            with self.assertRaises(ValueError):m.validate_scan(bad, dict(count=100, digest='aabb'))

    def test_disjoint_finite_scan_phases_are_required(self):
        for field, value in [('batch_seconds', 1.), ('sink_seconds', float('nan')),
                             ('open_seconds', -1.), ('seconds', float('inf')), ('first_seconds', 2.)]:
            bad=scan();bad['result'][field]=value
            with self.assertRaises(ValueError):m.validate_scan(bad, dict(count=100, digest='aabb'))

    def test_three_verified_trials_and_complete_index_phase_accounting(self):
        self.assertEqual(m.summarize(case())['cold_serial']['trials'], 3)
        mutations=[lambda c:c['trials'].pop(),
                   lambda c:c['trials'][0]['build']['result']['phases'][0].update(complete=False),
                   lambda c:c['trials'][0]['session']['result']['phases'][0].update(replay_seconds=2.),
                   lambda c:c['trials'][0]['index'].update(bytes=9999),
                   lambda c:c['trials'][0].update(source_identity_rechecked=False),
                   lambda c:c['trials'][0]['session']['result']['phases'][-1].update(digest='wrong')]
        for mutate in mutations:
            bad=case();mutate(bad)
            with self.assertRaises(ValueError):m.summarize(bad)

    def test_no_saving_has_no_break_even_or_invented_speedup(self):
        bad=case()
        for t in bad['trials']:
            for q in t['session']['result']['phases'][1:]:q['seconds']=2.
        result=m.summarize(bad)
        self.assertEqual(result['warm_speedup'], .5)
        self.assertIsNone(result['build_and_reopen_break_even_queries'])

    def test_missing_families_sizes_and_trials_cannot_complete_baseline(self):
        rows=[]
        for kind in m.KINDS:
            for n in m.counts_for(kind, 'small'):
                c=case(n);c['kind']=kind;rows.append(c)
        m.validate_cases(rows, 'small')
        for mutate in [lambda c:c.pop(), lambda c:c[-1].update(kind='records'),
                       lambda c:c[-1].update(count=c[-2]['count']),
                       lambda c:c[-1]['trials'].pop(),
                       lambda c:c[-1]['source'].update(bytes=0),
                       lambda c:c[-1]['expected_all'].update(count=1)]:
            bad=copy.deepcopy(rows);mutate(bad)
            with self.assertRaises(ValueError):m.validate_cases(bad, 'small')
