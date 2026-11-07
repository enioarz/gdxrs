"""Task 040: M6 ordered/cancelable parallel acceptance on oversized inputs (Linux)."""
import json
import math
import platform
import shutil
import statistics
import subprocess
import sys
import uuid
from pathlib import Path

import m5_scale as m5
import oracle
import scale
import writer_scale

ROOT = Path(__file__).resolve().parents[1]
SOURCES = sorted(set(m5.SOURCES + ['tools/m6_acceptance.py', 'examples/m6_acceptance_worker.rs',
                                   'tools/tests/test_m6_acceptance.py']))
TRIALS = 3
KINDS = ('records', 'compressed', 'metadata')
WORKERS = (2, 4, 8)


def counts_for(profile):
    return [4096, 16384] if profile == 'smoke' else [18_000_000, 36_000_000]


def expected_metadata_prefix(rows):
    digest = scale.INITIAL
    for n in range(1, rows + 1):
        digest = scale.step(digest, n)
        digest = scale.step(digest, 0)
    return dict(count=rows, digest=f'{digest:016x}')


def expected_prefix(kind, rows):
    return expected_metadata_prefix(rows) if kind == 'metadata' else scale.expected(rows)['all']


def validate_containment(trial):
    r = trial['result']
    top = {k: r.get(k, 0) for k in ['root_limit', 'root_peak', 'os_cap_verified', 'vm_peak_bytes']}
    top['os_cap_verified'] = r.get('os_cap_verified', False)
    vm_peak = r.get('vm_peak_bytes', 0)
    if (trial['exit_code'] or top['root_limit'] != m5.BUDGET
            or not 0 < top['root_peak'] <= m5.BUDGET or not top['os_cap_verified']
            or vm_peak > m5.CAP
            or trial['os_cap']['soft_bytes'] != m5.CAP or trial['os_cap']['hard_bytes'] != m5.CAP
            or not 0 < trial['peak_rss_bytes'] <= m5.CAP or trial['sampled_peak_fds'] > 6):
        raise ValueError('acceptance shared-root/process/descriptor containment missing')


def validate_serial(trial, expected):
    r = trial['result']
    if m5.identity(r) != expected:
        raise ValueError('serial ordered digest/count mismatch')
    validate_containment(trial)
    times = ['seconds', 'open_seconds', 'setup_seconds', 'first_seconds', 'batch_seconds',
             'sink_seconds', 'close_seconds']
    if any(not math.isfinite(r[k]) or r[k] < 0 for k in times) or r['seconds'] <= 0 \
            or sum(r[k] for k in ['open_seconds', 'setup_seconds', 'batch_seconds', 'sink_seconds', 'close_seconds']) > r['seconds'] + 1e-9 \
            or not 0 < r['first_seconds'] <= r['seconds'] or r['batches'] <= 0 \
            or r['decoded_rows'] != r['count'] or r['returned_rows'] != r['count']:
        raise ValueError('invalid serial phase accounting')


def validate_phase(p, workers, query, expected, kind):
    admitted = 0 if query == 'zero' else workers
    if (p['query'] != query or m5.identity(p) != expected or p['returned_rows'] != p['count']
            or p['requested'] != workers or p['admitted'] != admitted or p['joined'] != admitted
            or p['maximum_buffered_batches'] != admitted or p['peak_ready_batches'] > admitted
            or p['stack_reserved'] != admitted * (2 * 1024**2 + 64 * 1024)
            or p['control_reserved'] < admitted * 256 * 1024
            or p['complete'] != (query != 'cancel') or p['cancelled'] != (query == 'cancel')):
        raise ValueError('parallel phase admission/order/cancel mismatch')
    if query == 'all' and (p['decoded_rows'] != p['count'] or p['produced_rows'] != p['count']
                           or p['tasks_dispatched'] != p['tasks_completed']):
        raise ValueError('parallel full scan did not finish exactly')
    if query == 'zero' and (p['decoded_rows'] or p['produced_rows'] or p['tasks_dispatched']):
        raise ValueError('zero limit did record work')
    if query == 'cancel' and not 0 < p['count'] <= (65536 if kind == 'metadata' else 1024):
        raise ValueError('unbounded cancelled prefix')
    if any(not math.isfinite(p[k]) or p[k] < 0
           for k in ['seconds', 'first_seconds', 'batch_seconds', 'sink_seconds']):
        raise ValueError('invalid parallel phase accounting')
    if query == 'zero':
        if p['first_seconds'] != 0 or p['seconds'] < 0:
            raise ValueError('zero query must be lazy')
    elif p['seconds'] <= 0 or not 0 < p['first_seconds'] <= p['seconds'] \
            or p['batch_seconds'] + p['sink_seconds'] > p['seconds'] + 1e-9:
        raise ValueError('invalid parallel phase accounting')


def validate_parallel(trial, workers, query, expected, kind, phase_count=1):
    r = trial['result']
    if r['mode'] != 'parallel' or not math.isfinite(r['reopen_seconds']) or r['reopen_seconds'] < 0:
        raise ValueError('invalid parallel run')
    validate_containment(trial)
    if len(r['phases']) != phase_count or any(p['query'] != query for p in r['phases']):
        raise ValueError('parallel phase count/query mismatch')
    for p in r['phases']:
        validate_phase(p, workers, query, expected if query != 'cancel' else expected_prefix(kind, p['count']), kind)


def validate_trial(case, t):
    kind, n = case['kind'], case['count']
    for name in ['serial', 'build', 'session'] + [f'parallel-{w}' for w in WORKERS] \
            + ['parallel-cancel', 'parallel-head', 'parallel-zero', 'parallel-warm']:
        if name not in t:
            raise ValueError('missing trial phase ' + name)
        validate_containment(t[name])
    validate_serial(t['serial'], case['expected_all'])
    for name, expected_phases in [('build', 1), ('session', 2)]:
        # Replayed reports must enforce the same index admission as live runs.
        m5.validate_worker(t[name])
        phases = m5.phases(t[name])
        if len(phases) != expected_phases:
            raise ValueError('index phase count mismatch: ' + name)
        if any(not math.isfinite(p['seconds']) or p['seconds'] <= 0 for p in phases):
            raise ValueError('invalid index phase duration: ' + name)
    built = m5.phases(t['build'])[0]
    opened = m5.phases(t['session'])[0]
    indexed = m5.phases(t['session'])[1]
    if not built['complete'] or not built['published'] or built['rows_decoded'] != n:
        raise ValueError('incomplete explicit build')
    if not opened['complete'] or opened['source_bytes_hashed'] != case['source']['bytes']:
        raise ValueError('strict reopen validation incomplete')
    if m5.identity(indexed) != case['expected_all'] or indexed['query'] != 'all':
        raise ValueError('indexed full ordered digest mismatch')
    for w in WORKERS:
        validate_parallel(t[f'parallel-{w}'], w, 'all', case['expected_all'], kind)
    validate_parallel(t['parallel-cancel'], 4, 'cancel', None, kind)
    validate_parallel(t['parallel-head'], 2, 'head', expected_prefix(kind, 13), kind)
    validate_parallel(t['parallel-zero'], 2, 'zero', dict(count=0, digest=f'{scale.INITIAL:016x}'), kind)
    validate_parallel(t['parallel-warm'], 4, 'all', case['expected_all'], kind, phase_count=3)


def qualify(cases):
    for kind in KINDS:
        rows = [c for c in cases if c['kind'] == kind]
        if len(rows) < 2 or rows[-1]['count'] <= rows[0]['count']:
            raise ValueError('acceptance requires growing ' + kind + ' cases')
        last = rows[-1]
        if kind == 'compressed':
            if last['logical_bytes'] <= 4 * m5.CAP:
                raise ValueError('compressed logical input must exceed four times cap')
        else:
            if last['source']['bytes'] <= 4 * m5.CAP or last['logical_bytes'] <= 4 * m5.CAP:
                raise ValueError('physical AND logical input must exceed four times cap')
            if kind == 'metadata' and last['count'] * 64 <= 4 * m5.CAP:
                raise ValueError('metadata itself is not independently oversized')
        for c in rows:
            if len(c['trials']) != TRIALS:
                raise ValueError('three trials required per case')
            for t in c['trials']:
                validate_trial(c, t)
        worker_keys = ['serial'] + [f'parallel-{w}' for w in WORKERS] \
            + ['parallel-cancel', 'parallel-head', 'parallel-zero', 'parallel-warm']
        baseline = max(t[k]['peak_rss_bytes'] for t in rows[0]['trials'] for k in worker_keys)
        for t in last['trials']:
            for k in worker_keys:
                if t[k]['peak_rss_bytes'] > baseline + 64 * 1024**2:
                    raise ValueError('RSS growth exceeds fixed 64 MiB allowance')


def performance(case):
    # Do not publish costs from unvalidated trials, including report replays.
    for t in case['trials']:
        validate_trial(case, t)
    serial = [t['serial']['result']['seconds'] for t in case['trials']]
    out = dict(serial_seconds=m5.samples(serial),
               serial_phases={name: m5.samples([t['serial']['result'][name] for t in case['trials']])
                              for name in ['first_seconds', 'batch_seconds', 'sink_seconds']},
               build_seconds=m5.samples([m5.phases(t['build'])[0]['seconds'] for t in case['trials']]),
               reopen_seconds=m5.samples([m5.phases(t['session'])[0]['seconds'] for t in case['trials']]),
               indexed_all_seconds=m5.samples([m5.phases(t['session'])[1]['seconds'] for t in case['trials']]))
    indexed = [m5.phases(t['session'])[1]['seconds'] for t in case['trials']]
    parallel = {}
    for w in WORKERS:
        cold = [t[f'parallel-{w}']['result']['phases'][0]['seconds'] for t in case['trials']]
        s = m5.samples(cold)
        s['speedup_vs_serial'] = statistics.median(serial) / s['median'] if s['median'] > 0 else None
        s['speedup_vs_indexed'] = statistics.median(indexed) / s['median'] if s['median'] > 0 else None
        s['cache_state'] = 'parallel queries follow a strict source reopen in the same process (hash/replay warms the OS cache); cold serial uses advisory eviction - not like-for-like'
        s['phases'] = {name: m5.samples([t[f'parallel-{w}']['result']['phases'][0][name] for t in case['trials']])
                       for name in ['first_seconds', 'batch_seconds', 'sink_seconds']}
        parallel[str(w)] = s
    warm = [p['seconds'] for t in case['trials'] for p in t['parallel-warm']['result']['phases'][1:]]
    parallel['4-warm'] = m5.samples(warm)
    cancel = [t['parallel-cancel']['result']['phases'][0]['count'] for t in case['trials']]
    out['parallel'] = parallel
    out['cancelled_prefix_rows'] = dict(min=min(cancel), median=statistics.median(cancel), max=max(cancel))
    return out


def check(run, profile):
    if sys.platform != 'linux' or profile not in ['smoke', 'scale']:
        raise ValueError('m6-acceptance requires Linux smoke/scale')
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    hashes = {p: oracle.file_digest(ROOT / p) for p in SOURCES}
    if profile == 'scale' and subprocess.check_output(
            ['git', 'status', '--porcelain', '--', *SOURCES], cwd=ROOT, text=True).strip():
        raise ValueError('commit measured sources before M6 acceptance scale')
    area = ROOT / '.work/m6-acceptance' / uuid.uuid4().hex
    area.mkdir(parents=True)
    native, _, reference = writer_scale.build(run, area)
    run(['cargo', 'build', '--locked', '--release', '--features', 'index',
         '--example', 'm6_acceptance_worker', '--example', 'm5_worker'])
    bins = {}
    for name in ['m6_acceptance_worker', 'm5_worker']:
        bins[name] = area / name
        shutil.copyfile(ROOT / 'target/release/examples' / name, bins[name])
        bins[name].chmod(0o700)
    cases = []

    def stride_for(n):
        return 64 if n <= 65536 else 65536

    def progress():
        (area / 'progress.json').write_text(json.dumps(
            dict(source_revision=revision, source_hashes=hashes, cases=cases, m6_qualified=False), indent=2) + '\n')

    def call_worker(argv, name):
        t = scale.measure(argv, area, name, timeout=7200)
        return t

    for kind in KINDS:
        for n in counts_for(profile):
            source = area / f'{kind}-{n}.gdx'
            index = area / f'{kind}-{n}.index'
            estimate = n * (140 if kind != 'compressed' else 40) + 3 * 1024**3
            if shutil.disk_usage(area).free < estimate:
                raise ValueError('insufficient owned-acceptance scratch headroom')
            if kind == 'compressed':
                generated = json.loads(run([native, 'write', source, 'records', n, 'zlib'],
                                           capture=True, timeout=7200).stdout)
                recipe = dict(count=n, labels=generated['labels'], label_digest=generated['label_digest'],
                              expected=dict(all=generated['all']))
            else:
                recipe = json.loads(run([native, 'generate', source,
                                         'metadata' if kind == 'metadata' else 'records', n],
                                        capture=True, timeout=7200).stdout)
            if profile == 'smoke':
                if kind != 'metadata' and recipe['expected']['all'] != scale.expected(n)['all']:
                    raise ValueError('independent Python record calibration mismatch')
                if recipe['label_digest'] != scale.expected_labels(recipe['labels']):
                    raise ValueError('independent Python label calibration mismatch')
            identity = writer_scale.hash_record(source)
            command = ([native, 'native-universe', source, 'metadata', n] if kind == 'metadata'
                       else [native, 'native', source, 'records', n])
            validation = writer_scale.measure(command, area, f'{kind}-{n}-native',
                                              reference_metadata=kind == 'metadata')
            if kind == 'metadata':
                if validation['result']['count'] != n or validation['result']['label_digest'] != recipe['label_digest']:
                    raise ValueError('native metadata validation mismatch')
                expected_all = dict(count=n, digest=validation['result']['digest'])
                labels = call_worker([bins['m5_worker'], 'labels', source, index, 'metadata', n, stride_for(n), 'all', 0],
                                     f'{kind}-{n}-labels')
                m5.validate_worker(labels)
                if m5.identity(m5.phases(labels)[0]) != dict(count=n, digest=recipe['label_digest']):
                    raise ValueError('full Rust/native label stream mismatch')
            else:
                if validation['result']['all'] != recipe['expected']['all']:
                    raise ValueError('native full record validation mismatch')
                expected_all = recipe['expected']['all']
            case = dict(kind=kind, count=n, source=identity, logical_bytes=n * (64 if kind == 'metadata' else 120),
                        recipe=recipe, native_validation=validation, expected_all=expected_all, trials=[])
            cases.append(case)
            progress()
            for trial in range(TRIALS):
                name = f'{kind}-{n}-{trial}'
                t = {}
                case['trials'].append(t)
                scale.evict(source)
                t['serial'] = call_worker([bins['m6_acceptance_worker'], 'serial', source,
                                           'metadata' if kind == 'metadata' else 'records', n], name + '-serial')
                scale.evict(source)
                t['build'] = call_worker([bins['m5_worker'], 'build', source, index,
                                          'metadata' if kind == 'metadata' else 'records', n, stride_for(n), 'all', 0],
                                         name + '-build')
                m5.validate_worker(t['build'])
                with index.open('rb') as f:
                    header = f.read(1024)
                if header[24:56].hex() != identity['sha256']:
                    raise ValueError('independent source/header SHA-256 mismatch')
                scale.evict(source)
                scale.evict(index)
                t['session'] = call_worker([bins['m5_worker'], 'session', source, index,
                                            'metadata' if kind == 'metadata' else 'records', n, stride_for(n), 'all', 0],
                                           name + '-session')
                m5.validate_worker(t['session'])
                for w in WORKERS:
                    scale.evict(source)
                    t[f'parallel-{w}'] = call_worker([bins['m6_acceptance_worker'], 'parallel', source, index,
                                                      'metadata' if kind == 'metadata' else 'records', n, stride_for(n), w, 'all', 0],
                                                     f'{name}-p{w}')
                scale.evict(source)
                t['parallel-cancel'] = call_worker([bins['m6_acceptance_worker'], 'parallel', source, index,
                                                    'metadata' if kind == 'metadata' else 'records', n, stride_for(n), 4, 'cancel', 0],
                                                   name + '-cancel')
                scale.evict(source)
                t['parallel-head'] = call_worker([bins['m6_acceptance_worker'], 'parallel', source, index,
                                                  'metadata' if kind == 'metadata' else 'records', n, stride_for(n), 2, 'head', 0],
                                                 name + '-head')
                scale.evict(source)
                t['parallel-zero'] = call_worker([bins['m6_acceptance_worker'], 'parallel', source, index,
                                                  'metadata' if kind == 'metadata' else 'records', n, stride_for(n), 2, 'zero', 0],
                                                 name + '-zero')
                scale.evict(source)
                t['parallel-warm'] = call_worker([bins['m6_acceptance_worker'], 'parallel', source, index,
                                                  'metadata' if kind == 'metadata' else 'records', n, stride_for(n), 4, 'all', 2],
                                                 name + '-warm')
                if writer_scale.hash_record(source) != identity:
                    raise ValueError('acceptance run mutated source')
                validate_trial(case, t)
                progress()
                index.unlink()
            if writer_scale.hash_record(source) != identity:
                raise ValueError('M6 acceptance mutated GDX source')
            if profile == 'scale':
                case['performance'] = performance(case)
            progress()
            source.unlink()
    special_values = None
    if profile == 'smoke':
        fixture = ROOT / 'tests/fixtures/v7/semantics-0.gdx'
        expected_tsv = fixture.with_suffix('.expected.tsv').read_text()
        fixture_count = sum(1 for line in expected_tsv.splitlines()
                            if line.startswith('R	') and line.split('	')[1] == '7')
        special_index = area / 'special-semantics-0.index'
        t = scale.measure([bins['m6_acceptance_worker'], 'special', fixture, special_index, '7', fixture_count],
                          area, 'special-values', timeout=600)
        validate_containment(t)
        r = t['result']
        if r['mode'] != 'special' or r['count'] != fixture_count or r['specials'] <= 0:
            raise ValueError('special-value case missing sentinel/acronym values')
        digests = {m['digest'] for m in r['modes']}
        if len(r['modes']) != 3 or len(digests) != 1:
            raise ValueError('serial/indexed/parallel special-value digests differ')
        special_index.unlink()
        special_values = dict(fixture=str(fixture.relative_to(ROOT)), symbol=7, count=fixture_count,
                              specials=r['specials'], modes=r['modes'])
    if profile == 'scale':
        qualify(cases)
    if hashes != {p: oracle.file_digest(ROOT / p) for p in SOURCES}:
        raise ValueError('measured sources changed during acceptance')
    return dict(source_revision=revision, source_hashes=hashes, profile=profile,
                m6_qualified=profile == 'scale', trials=TRIALS, workers=list(WORKERS), special_values=special_values,
                root=str(area.relative_to(ROOT)), cases=cases, oracle=reference,
                binary_hashes={k: oracle.file_digest(p) for k, p in bins.items()},
                native_binary_sha256=oracle.file_digest(native),
                compiler=run(['rustc', '--version', '--verbose'], capture=True).stdout,
                hardware=dict(platform=platform.platform(), cpu=next(
                    line.split(':', 1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines()
                    if line.startswith('model name'))), cpus=__import__('os').cpu_count(),
                storage=run(['df', '-T', ROOT], capture=True).stdout,
                library_budget=m5.BUDGET, rust_process_cap=m5.CAP,
                cache_policy='Fresh OS-capped process per phase with advisory per-file fsync/FADV_DONTNEED; device caches uncontrolled; warm parallel repeats reuse one validated session in-process',
                limitations=['Linux RLIMIT_AS/RSS containment and timings only; not cross-platform',
                             'Compressed cases qualify logical oversized records; physical compressed files may fit RAM',
                             'Parallel speedup is workload/host specific; a serial fallback is not parallel benefit',
                             'No independent approval, automatic M6 release or general throughput parity'])
