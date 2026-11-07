"""Task 039: measured explicit scan policy, end-to-end comparisons (Linux only)."""
import json
import math
import os
import platform
import shutil
import subprocess
import sys
import uuid
from pathlib import Path

import m5_scale as m5
import oracle
import scale
import writer_scale

ROOT = Path(__file__).resolve().parents[1]
SOURCES = sorted(set(m5.SOURCES + ['tools/m6_policy.py', 'examples/m6_policy_worker.rs',
                                   'tools/tests/test_m6_policy.py']))
KINDS = ('records', 'compressed', 'metadata')
TOLERANCE = 1.5


def counts_for(profile):
    return [4096, 16384] if profile == 'smoke' else [1_000_000, 4_000_000]


def validate_containment(trial):
    r = trial['result']
    if (trial['exit_code'] or r['root_limit'] != m5.BUDGET or not 0 < r['root_peak'] <= m5.BUDGET
            or not r['os_cap_verified'] or trial['os_cap']['soft_bytes'] != m5.CAP
            or trial['os_cap']['hard_bytes'] != m5.CAP
            or not 0 < trial['peak_rss_bytes'] <= m5.CAP or not 0 < r['vm_peak_bytes'] <= m5.CAP
            or trial['sampled_peak_fds'] > 6):
        raise ValueError('policy worker shared-root/process/descriptor containment missing')


def validate_case(case):
    r = case['result']
    for field in ['serial_seconds', 'build_seconds', 'reopen_seconds', 'indexed_seconds',
                  'leading_seconds', 'parallel_seconds', 'index_intervals',
                  'parallel_tasks_dispatched', 'shapes']:
        if field not in r:
            raise ValueError('missing measured field ' + field)
    if len(r['parallel_seconds']) != 3 or not all(math.isfinite(v) and v > 0 for v in
               [r['serial_seconds'], r['build_seconds'], r['reopen_seconds'],
                r['indexed_seconds'], r['leading_seconds'], *r['parallel_seconds']]):
        raise ValueError('invalid policy measurement')
    if (r['index_intervals'] <= 0 or len(r['parallel_tasks_dispatched']) != 3
            or any(tasks != r['index_intervals'] for tasks in r['parallel_tasks_dispatched'])):
        raise ValueError('parallel calibration must use one interval per task')
    if r['parallel_digest'] != r['serial_digest'] or r['count'] != case['count']:
        raise ValueError('parallel/serial ordered digest mismatch')
    shapes = {s['shape']: s for s in r['shapes']}
    if len(r['shapes']) != 4 or set(shapes) != {'all', 'leading', 'head', 'zero'}:
        raise ValueError('all four policy query shapes are mandatory')
    for name, s in shapes.items():
        if (not math.isfinite(s['seconds']) or s['seconds'] < 0
                or (name != 'zero' and s['seconds'] == 0)):
            raise ValueError('invalid planned duration: ' + name)
        if s['count'] != {'all': case['count'], 'leading': r['leading_count'],
                          'head': min(13, case['count']), 'zero': 0}[name]:
            raise ValueError('planned shape count mismatch: ' + name)
    for name in ['head', 'zero']:
        if shapes[name]['choice'] != 'serial':
            raise ValueError('conservative serial required for ' + name)
    for name, measured in [('all', r['indexed_seconds']), ('leading', r['leading_seconds'])]:
        best = min(r['serial_seconds'], measured, *r['parallel_seconds'])
        if shapes[name]['seconds'] > TOLERANCE * best:
            raise ValueError('planned ' + name + ' choice is not within tolerance of the measured best')
    for name, digest in [('all', r['serial_digest']), ('leading', r['leading_digest'])]:
        if shapes[name]['digest'] != digest:
            raise ValueError('planned ' + name + ' ordered digest mismatch')


def qualify(cases):
    for kind in KINDS:
        rows = [c for c in cases if c['kind'] == kind]
        if len(rows) < 2 or rows[-1]['count'] <= rows[0]['count']:
            raise ValueError('policy requires growing ' + kind + ' cases')
        for c in rows:
            validate_case(c)
            if max(t['peak_rss_bytes'] for t in c['trials']) > m5.CAP:
                raise ValueError('policy RSS exceeds process cap')


def validate_and_retire(case, source, index):
    """Keep owned inputs available until their result and identity checks pass."""
    validate_case(case)
    if writer_scale.hash_record(source) != case['source']:
        raise ValueError('policy run mutated source')
    source.unlink()
    index.unlink()


def check(run, profile):
    if sys.platform != 'linux' or profile not in ['smoke', 'small']:
        raise ValueError('m6-policy requires Linux smoke/small')
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    hashes = {p: oracle.file_digest(ROOT / p) for p in SOURCES}
    if profile == 'small' and subprocess.check_output(
            ['git', 'status', '--porcelain', '--', *SOURCES], cwd=ROOT, text=True).strip():
        raise ValueError('commit measured sources before small policy comparisons')
    area = ROOT / '.work/m6-policy' / uuid.uuid4().hex
    area.mkdir(parents=True)
    native, _, reference = writer_scale.build(run, area)
    run(['cargo', 'build', '--locked', '--release', '--features', 'index', '--example', 'm6_policy_worker'])
    worker = area / 'm6_policy_worker'
    shutil.copyfile(ROOT / 'target/release/examples/m6_policy_worker', worker)
    worker.chmod(0o700)
    cases = []

    def progress():
        (area / 'progress.json').write_text(json.dumps(
            dict(source_revision=revision, source_hashes=hashes, cases=cases,
                 policy_qualified=False), indent=2) + '\n')

    for kind in KINDS:
        for n in counts_for(profile):
            source = area / f'{kind}-{n}.gdx'
            index = area / f'{kind}-{n}.index'
            estimate = n * 140
            if shutil.disk_usage(area).free < estimate + 2 * 1024**3:
                raise ValueError('insufficient owned-policy scratch headroom')
            if kind == 'compressed':
                generated = json.loads(run([native, 'write', source, 'records', n, 'zlib'],
                                           capture=True, timeout=7200).stdout)
                recipe = dict(count=n, labels=generated['labels'],
                              label_digest=generated['label_digest'],
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
            elif validation['result']['all'] != recipe['expected']['all']:
                raise ValueError('native full record validation mismatch')
            case = dict(kind=kind, count=n, source=identity, recipe=recipe,
                        native_validation=validation, trials=[])
            cases.append(case)
            progress()
            scale.evict(source)
            trial = scale.measure([worker, source, index,
                                   'metadata' if kind == 'metadata' else 'records', n],
                                  area, f'{kind}-{n}-policy', timeout=7200)
            case['trials'].append(trial)
            case['result'] = trial['result']
            progress()
            validate_containment(trial)
            validate_and_retire(case, source, index)
    qualify(cases)
    if hashes != {p: oracle.file_digest(ROOT / p) for p in SOURCES}:
        raise ValueError('measured sources changed during policy comparisons')
    return dict(source_revision=revision, source_hashes=hashes, profile=profile,
                policy_qualified=profile == 'small', tolerance=TOLERANCE,
                root=str(area.relative_to(ROOT)), cases=cases, oracle=reference,
                worker_sha256=oracle.file_digest(worker), native_binary_sha256=oracle.file_digest(native),
                compiler=run(['rustc', '--version', '--verbose'], capture=True).stdout,
                hardware=dict(platform=platform.platform(), cpu=next(
                    line.split(':', 1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines()
                    if line.startswith('model name'))), cpus=os.cpu_count(),
                storage=run(['df', '-T', ROOT], capture=True).stdout,
                library_budget=m5.BUDGET, rust_process_cap=m5.CAP,
                cache_policy='Fresh OS-capped process per case with advisory per-file fsync/FADV_DONTNEED; device caches uncontrolled',
                limitations=['Linux RLIMIT_AS/RSS containment only; timings are not cross-platform',
                             'One measured comparison per case/shape; dispersion comes from the smoke/small pair and task 036 trials',
                             'Policy inputs are caller-supplied measurements; the planner itself performs no I/O',
                             'No automatic release, universal threshold or hidden cache is claimed'])
