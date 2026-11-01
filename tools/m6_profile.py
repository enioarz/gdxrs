"""Task 036: diagnostic wall-clock phase profiles; no parallel speedup claim."""
import json
import math
import platform
from pathlib import Path
import shutil
import statistics
import subprocess
import sys
import uuid

import m5_scale as m5
import oracle
import scale
import writer_scale

ROOT = Path(__file__).resolve().parents[1]
SOURCES = sorted(set(m5.SOURCES + ['tools/m6_profile.py', 'examples/m6_profile_worker.rs',
                                  'tools/tests/test_m6_profile.py']))
TRIALS = 3
TIMES = ['open_seconds', 'setup_seconds', 'batch_seconds', 'sink_seconds', 'close_seconds']


def validate_scan(trial, expected):
    r = trial['result']
    if trial['exit_code'] or m5.identity(r) != expected:
        raise ValueError('profile full ordered digest/count mismatch')
    if (r['root_limit'] != m5.BUDGET or not 0 < r['root_peak'] <= m5.BUDGET
            or not r['os_cap_verified'] or trial['os_cap']['soft_bytes'] != m5.CAP
            or trial['os_cap']['hard_bytes'] != m5.CAP
            or not 0 < trial['peak_rss_bytes'] <= m5.CAP
            or trial['sampled_peak_fds'] > 4):
        raise ValueError('profile shared-root/process/descriptor containment mismatch')
    if any(not math.isfinite(r[k]) or r[k] < 0 for k in TIMES + ['seconds', 'first_seconds']):
        raise ValueError('invalid phase duration')
    # Each timed region is disjoint; allow only float-roundoff, never a percentage
    # of runtime that could hide actual overlapping/invented phase accounting.
    if (r['seconds'] <= 0 or sum(r[k] for k in TIMES) > r['seconds'] + 1e-9
            or not 0 < r['first_seconds'] <= r['seconds'] or r['batches'] <= 0
            or r['decoded_rows'] != r['count'] or r['returned_rows'] != r['count']):
        raise ValueError('incomplete scan/impossible phase accounting')


def validate_trial(case, t):
    for name in ['cold_serial', 'build', 'session', 'serial_matrix']:
        m5.validate_worker(t[name])
    first = m5.compare_queries(t['serial_matrix'], t['session'], t['cold_serial'], case['source']['bytes'])
    built, opened = m5.phases(t['build'])[0], m5.phases(t['session'])[0]
    if (not built['complete'] or not built['published'] or built['rows_decoded'] != case['count']
            or built['index_bytes'] != t['index']['bytes'] or opened['index_bytes'] != t['index']['bytes']
            or built['index_bytes'] > m5.QUOTA or not t['source_identity_rechecked']):
        raise ValueError('incomplete build/index extent/source mismatch')
    for phase in [built, opened]:
        if (any(not math.isfinite(phase[k]) or phase[k] < 0
                for k in ['seconds', 'digest_seconds', 'replay_seconds'])
                or phase['digest_seconds'] + phase['replay_seconds'] > phase['seconds'] + 1e-9):
            raise ValueError('impossible index phase accounting')
    for worker in [t['cold_serial'], t['serial_matrix'], t['session']]:
        for q in m5.phases(worker):
            if not math.isfinite(q['seconds']) or q['seconds'] < 0:
                raise ValueError('invalid query duration')
    all_rows = next(r for r in m5.phases(t['session'])[1:] if r['query'] == 'all')
    if m5.identity(all_rows) != case['expected_all']:
        raise ValueError('native/indexed full ordered result mismatch')
    if case['kind'] != 'metadata':
        validate_scan(t['profile_scan'], case['expected_all'])
    return first, built, opened


def summarize(case):
    rows = case['trials']
    if len(rows) != TRIALS:
        raise ValueError('exactly three complete phase trials required')
    values = {name: [] for name in ['cold_serial', 'build', 'strict_reopen',
                                   'source_digest', 'canonical_replay', 'first_indexed', 'warm']}
    for t in rows:
        first, built, opened = validate_trial(case, t)
        values['cold_serial'].append(m5.phases(t['cold_serial'])[0]['seconds'])
        values['build'].append(built['seconds'])
        values['strict_reopen'].append(opened['seconds'])
        values['source_digest'].append(opened['digest_seconds'])
        values['canonical_replay'].append(opened['replay_seconds'])
        values['first_indexed'].append(first[0]['seconds'])
        values['warm'].extend(q['seconds'] for q in first[1:])
    result = {name: m5.samples(observations) for name, observations in values.items()}
    cold, warm = result['cold_serial']['median'], result['warm']['median']
    saving = cold - warm
    result['warm_speedup'] = cold / warm if warm > 0 else None
    # Negative/zero savings are a legitimate profiling result, never a speedup pass.
    result['build_and_reopen_break_even_queries'] = (
        max(1, math.ceil((result['build']['median'] + result['strict_reopen']['median']
                         + result['first_indexed']['median'] - warm) / saving)) if saving > 0 else None)
    if case['kind'] != 'metadata':
        result['full_scan'] = {name: m5.samples([t['profile_scan']['result'][name] for t in rows])
                               for name in TIMES + ['seconds', 'first_seconds']}
        result['batch_fraction'] = statistics.median(
            t['profile_scan']['result']['batch_seconds'] / t['profile_scan']['result']['seconds'] for t in rows)
        result['sink_fraction'] = statistics.median(
            t['profile_scan']['result']['sink_seconds'] / t['profile_scan']['result']['seconds'] for t in rows)
    return result


def check(run, profile):
    if sys.platform != 'linux' or profile not in ['smoke', 'small']:
        raise ValueError('M6 diagnostic profiles require Linux smoke/small')
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if profile == 'small' and subprocess.check_output(
            ['git', 'status', '--porcelain', '--', *SOURCES], cwd=ROOT, text=True).strip():
        raise ValueError('commit measured sources before small baseline')
    hashes = {p: oracle.file_digest(ROOT / p) for p in SOURCES}
    area = ROOT / '.work/m6-profile' / uuid.uuid4().hex
    area.mkdir(parents=True)
    native, _, reference = writer_scale.build(run, area)
    run(['cargo', 'build', '--locked', '--release', '--features', 'index',
         '--example', 'm5_worker', '--example', 'm6_profile_worker'])
    bins = {}
    for name in ['m5_worker', 'm6_profile_worker']:
        bins[name] = area / name
        shutil.copyfile(ROOT / 'target/release/examples' / name, bins[name])
        bins[name].chmod(0o700)
    cases = []

    def progress():
        (area / 'progress.json').write_text(json.dumps(dict(source_revision=revision, cases=cases,
                                                           baseline_complete=False), indent=2) + '\n')

    def call(mode, source, index, kind, n, stride, selector, name, repeats=0):
        t = scale.measure([bins['m5_worker'], mode, source, index,
                           'metadata' if kind == 'metadata' else 'records', n, stride, selector, repeats], area, name)
        m5.validate_worker(t)
        return t

    for kind in ['records', 'compressed', 'metadata', 'directory']:
        counts = [4096, 16384] if profile == 'smoke' else (
            [131072, 524288] if kind == 'directory' else [1_000_000, 4_000_000])
        for n in counts:
            source, index = area / f'{kind}-{n}.gdx', area / f'{kind}-{n}.index'
            stride = 1 if kind == 'directory' else 65536
            estimate = n * (1300 if kind == 'directory' else 140)
            if shutil.disk_usage(area).free < estimate + 2 * 1024**3:
                raise ValueError('insufficient owned-profile scratch headroom')
            if kind == 'compressed':
                generated = json.loads(run([native, 'write', source, 'records', n, 'zlib'], capture=True).stdout)
                recipe = dict(count=n, labels=generated['labels'], label_digest=generated['label_digest'],
                              expected=dict(all=generated['all']), native_generation=generated)
            else:
                recipe = json.loads(run([native, 'generate', source,
                                         'metadata' if kind == 'metadata' else 'records', n], capture=True).stdout)
            if profile == 'smoke':
                if recipe['label_digest'] != scale.expected_labels(recipe['labels']):
                    raise ValueError('independent Python/native label calibration mismatch')
                if kind != 'metadata' and recipe['expected']['all'] != scale.expected(n)['all']:
                    raise ValueError('independent Python/native full record calibration mismatch')
            identity = writer_scale.hash_record(source)
            command = [native, 'native-universe', source, 'metadata', n] if kind == 'metadata' else [native, 'native', source, 'records', n]
            validation = writer_scale.measure(command, area, f'{kind}-{n}-native', reference_metadata=kind == 'metadata')
            if kind == 'metadata':
                actual = validation['result']
                if actual['count'] != n or actual['label_digest'] != recipe['label_digest']:
                    raise ValueError('native full metadata validation mismatch')
                expected = m5.identity(actual)
                labels = call('labels', source, index, kind, n, stride, 'all', f'{kind}-{n}-labels')
                if m5.identity(m5.phases(labels)[0]) != dict(count=n, digest=recipe['label_digest']):
                    raise ValueError('full Rust/native label stream mismatch')
            else:
                if (validation['result']['all'] != recipe['expected']['all']
                        or validation['result']['label_digest'] != recipe['label_digest']):
                    raise ValueError('native full record/label validation mismatch')
                expected, labels = recipe['expected']['all'], None
            case = dict(kind=kind, count=n, source=identity, recipe=recipe, native_validation=validation,
                        labels=labels, expected_all=expected, stride=stride, trials=[])
            cases.append(case)
            progress()
            for i in range(TRIALS):
                name = f'{kind}-{n}-{i}'
                t = {}
                case['trials'].append(t)
                if kind != 'metadata':
                    scale.evict(source)
                    t['profile_scan'] = scale.measure([bins['m6_profile_worker'], source, n], area, name + '-profile')
                    validate_scan(t['profile_scan'], expected)
                scale.evict(source)
                t['cold_serial'] = call('serial', source, index, kind, n, stride, 'leading', name + '-cold')
                scale.evict(source)
                t['build'] = call('build', source, index, kind, n, stride, 'all', name + '-build')
                with index.open('rb') as f:
                    header = f.read(1024)
                if header[24:56].hex() != identity['sha256']:
                    raise ValueError('independent source/header SHA-256 mismatch')
                scale.evict(source)
                scale.evict(index)
                t['session'] = call('session', source, index, kind, n, stride,
                                    'suite-directory' if kind == 'directory' else 'suite', name + '-session', 3)
                scale.evict(source)
                t['serial_matrix'] = call('serial-matrix', source, index, kind, n, stride,
                                          'directory' if kind == 'directory' else 'all', name + '-serial')
                t['index'] = writer_scale.hash_record(index)
                t['source_identity_rechecked'] = writer_scale.hash_record(source) == identity
                # All result, timing, resource and source checks precede retirement.
                validate_trial(case, t)
                progress()
                index.unlink()
            case['summary'] = summarize(case)
            if writer_scale.hash_record(source) != identity:
                raise ValueError('profile source changed')
            progress()
            source.unlink()
    if hashes != {p: oracle.file_digest(ROOT / p) for p in SOURCES}:
        raise ValueError('measured sources changed during profiling')
    return dict(source_revision=revision, source_hashes=hashes, profile=profile,
                baseline_complete=profile == 'small', parallel_qualified=False,
                root=str(area.relative_to(ROOT)), cases=cases, oracle=reference,
                binary_hashes={**{name: oracle.file_digest(p) for name, p in bins.items()},
                               'native': oracle.file_digest(native)},
                compiler=run(['rustc', '--version', '--verbose'], capture=True).stdout,
                hardware=dict(platform=platform.platform(), cpu=next(
                    line.split(':', 1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines()
                    if line.startswith('model name'))), storage=run(['df', '-T', ROOT], capture=True).stdout,
                library_budget=m5.BUDGET, rust_process_cap=m5.CAP,
                native_metadata_reference_cap=writer_scale.NATIVE_METADATA_CAP,
                cache_policy='Fresh processes and advisory per-file fsync/FADV_DONTNEED before source phases; first indexed query follows strict replay; three warm queries reuse the session. Device caches uncontrolled.',
                limitations=['Diagnostic phases include full sink verification; batch production includes I/O, decoding and column assembly, not a sampled function profile',
                             'Instant per batch adds instrumentation overhead; no subtraction or count-only performance claim',
                             'Small profiles are not independent oversized acceptance or parallel speedup qualification',
                             'Source read counters are per single-threaded worker; future concurrent deltas require separate accounting',
                             'Native eager metadata reference has a separate cap; Linux timings/RSS are not cross-platform measurements'])
