"""Task 035: source-bound, OS-capped persistent slice acceptance (Linux only)."""
import json, math, platform, shutil, statistics, subprocess, sys, uuid
from pathlib import Path
import oracle, scale, writer_scale
ROOT = Path(__file__).resolve().parents[1]
CAP = 512 * 1024 ** 2
BUDGET = 256 * 1024 ** 2
QUOTA = 12 * 1024 ** 3
TRIALS = 3
SOURCES = sorted(set(['tools/m5_scale.py', 'tools/dev.py', 'tools/scale.py', 'tools/writer_scale.py', 'tools/scale/driver.cpp', 'tools/scale/CMakeLists.txt', 'examples/m5_worker.rs', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'] + [str(p.relative_to(ROOT)) for p in (ROOT / 'src').rglob('*.rs')]))

def validate_worker(trial):
    r = trial['result']
    if r['root_limit'] != BUDGET or r['root_peak'] > BUDGET or (not r['os_cap_verified']):
        raise ValueError('M5 shared root/cap missing')
    if trial['os_cap']['soft_bytes'] != CAP or trial['os_cap']['hard_bytes'] != CAP or (not 0 < trial['peak_rss_bytes'] <= CAP):
        raise ValueError('M5 enforced process cap missing')
    if r['index_quota'] != QUOTA or r['handle_limit'] != 2 or r['cache_limit'] != 1024:
        raise ValueError('M5 index byte/handle/cache admission mismatch')
    if not 1 <= len(r['phases']) <= 13:
        raise ValueError('M5 bounded phase output missing')

def identity(row):
    return {k: row[k] for k in ['count', 'digest']}

def phases(trial):
    return trial['result']['phases']

def samples(rows):
    return dict(min=min(rows), median=statistics.median(rows), max=max(rows), trials=len(rows))

def compare_queries(serial, indexed, cold, source_bytes):
    rows = phases(indexed)
    opened = rows[0]
    if not opened['complete'] or opened['source_bytes_hashed'] != source_bytes:
        raise ValueError('strict identity validation incomplete')
    leading = [r for r in rows[1:] if r['query'] == 'leading']
    if len(leading) != 4 or any((identity(r) != identity(phases(cold)[0]) for r in leading)):
        raise ValueError('first/warm ordered leading digests differ')
    for row in rows[1:]:
        if row['returned_rows'] != row['count'] or row['validation_bytes_hashed'] != source_bytes or row['validation_pages'] != opened['pages_read']:
            raise ValueError('warm query validation/accounting drift')
    for row in phases(serial):
        found = [r for r in rows[1:] if r['query'] == row['query']]
        if not found or any((identity(r) != identity(row) for r in found)):
            raise ValueError('full serial/indexed ordered digest mismatch')
        if row['query'] == 'zero' and (row['decoded_rows'] or found[0]['decoded_rows'] or found[0]['bytes_read']):
            raise ValueError('zero limit is not lazy')
        if row['query'] == 'miss' and found[0]['decoded_rows']:
            raise ValueError('proven nonleading miss decoded records')
    return leading

def performance(case):
    cold = [phases(t['cold_serial'])[0] for t in case['trials']]
    leading = [[r for r in phases(t['session'])[1:] if r['query'] == 'leading'] for t in case['trials']]
    warm = [r for rows in leading for r in rows[1:]]
    'Require >=2x latency and >=4x complete-source-work reductions.'
    if statistics.median((r['seconds'] for r in warm)) > statistics.median((r['seconds'] for r in cold)) / 2:
        raise ValueError('narrow warm latency does not improve by 2x')
    for rows, serial in zip(leading, cold):
        for r in rows:
            if r['decoded_rows'] > serial['decoded_rows'] / 4:
                raise ValueError('narrow decoded rows not reduced by 4x')
            if case['kind'] != 'metadata':
                for field in ['bytes_read', 'decoded_bytes', 'decoded_buffers']:
                    if r[field] + (1024 * r['index_pages_read'] if field == 'bytes_read' else 0) > serial[field] / 4:
                        raise ValueError('narrow complete physical/index or decoded work not reduced by 4x: ' + field)
    builds = [phases(t['build'])[0]['seconds'] for t in case['trials']]
    opens = [phases(t['session'])[0] for t in case['trials']]
    first = [rows[0]['seconds'] for rows in leading]
    warm_seconds = [r['seconds'] for r in warm]
    serial_seconds = [r['seconds'] for r in cold]
    saving = statistics.median(serial_seconds) - statistics.median(warm_seconds)
    return dict(cold_serial_seconds=samples(serial_seconds), build_seconds=samples(builds), strict_reopen_seconds=samples([r['seconds'] for r in opens]), source_digest_seconds=samples([r['digest_seconds'] for r in opens]), canonical_replay_seconds=samples([r['replay_seconds'] for r in opens]), first_indexed_seconds=samples(first), warm_seconds=samples(warm_seconds), warm_speedup=statistics.median(serial_seconds) / statistics.median(warm_seconds), reopen_break_even_queries=math.ceil((statistics.median((r['seconds'] for r in opens)) + statistics.median(first) - statistics.median(warm_seconds)) / saving), build_and_reopen_break_even_queries=math.ceil((statistics.median(builds) + statistics.median((r['seconds'] for r in opens)) + statistics.median(first) - statistics.median(warm_seconds)) / saving))

def qualify(cases):
    for kind in ['records', 'metadata', 'directory']:
        rows = [c for c in cases if c['kind'] == kind]
        if len(rows) < 2 or rows[-1]['count'] <= rows[0]['count']:
            raise ValueError('M5 requires growing independently oversized ' + kind)
        for c in rows:
            if len(c['trials']) < 3:
                raise ValueError('M5 requires three independent build/reopen/cold trials')
            for t in c['trials']:
                for name in ['cold_serial', 'build', 'session', 'serial_matrix']:
                    validate_worker(t[name])
        last = rows[-1]
        if kind == 'directory':
            if min((phases(t['build'])[0]['directory_bytes'] for t in last['trials'])) <= CAP:
                raise ValueError('directory itself must exceed process cap')
            if min((t['index']['bytes'] for t in last['trials'])) <= 4 * CAP:
                raise ValueError('physical/logical sidecar input must exceed four times cap')
        else:
            if last['source']['bytes'] <= 4 * CAP or last['logical_bytes'] <= 4 * CAP:
                raise ValueError('physical AND logical source input must exceed four times cap')
            if kind == 'metadata' and last['count'] * 64 <= 4 * CAP:
                raise ValueError('metadata itself is not independently oversized')
        for phase in ['build', 'session', 'cold_serial', 'serial_matrix']:
            if max((t[phase]['peak_rss_bytes'] for t in last['trials'])) > max((t[phase]['peak_rss_bytes'] for t in rows[0]['trials'])) + 64 * 1024 ** 2:
                raise ValueError('M5 RSS growth exceeds fixed 64 MiB allowance')
        for c in rows:
            performance(c)

def check(run, profile):
    if sys.platform != 'linux' or profile not in ['smoke', 'scale']:
        raise ValueError('M5 acceptance requires Linux smoke/scale')
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    hashes = {p: oracle.file_digest(ROOT / p) for p in SOURCES}
    if profile == 'scale' and subprocess.check_output(['git', 'status', '--porcelain', '--', *SOURCES], text=True).strip():
        raise ValueError('commit measured sources before M5 scale')
    area = ROOT / '.work/m5-acceptance' / uuid.uuid4().hex
    area.mkdir(parents=True)
    native, _, reference = writer_scale.build(run, area)
    run(['cargo', 'build', '--locked', '--release', '--features', 'index', '--example', 'm5_worker'])
    worker = area / 'm5_worker'
    shutil.copyfile(ROOT / 'target/release/examples/m5_worker', worker)
    worker.chmod(448)
    cases = []
    calibration = []

    def progress():
        (area / 'progress.json').write_text(json.dumps(dict(source_revision=revision, source_hashes=hashes, cases=cases, calibration=calibration, m5_qualified=False), indent=2) + '\n')

    def call(mode, source, index, kind, n, stride, selector, name, repeats=0):
        t = scale.measure([worker, mode, source, index, 'metadata' if kind == 'metadata' else 'records', n, stride, selector, repeats], area, name, timeout=7200)
        validate_worker(t)
        return t
    schedule = [('records', n) for n in ([4096, 16384] if profile == 'smoke' else [18000000, 36000000])] + [('metadata', n) for n in ([4096, 16384] if profile == 'smoke' else [18000000, 36000000])] + [('directory', n) for n in ([4096, 16384] if profile == 'smoke' else [4200000, 8400000])]
    progress()
    for kind, n in schedule:
        stride = 1 if kind == 'directory' else 65536
        estimated = n * (126 + 1100) if kind == 'directory' else n * (65 if kind == 'metadata' else 127)
        if shutil.disk_usage(area).free < estimated + 2 * 1024 ** 3:
            raise ValueError('insufficient storage for owned M5 case; no unrelated artifacts removed')
        source = area / f'{kind}-{n}.gdx'
        index = area / f'{kind}-{n}.index'
        recipe = json.loads(run([native, 'generate', source, 'metadata' if kind == 'metadata' else 'records', n], capture=True, timeout=7200).stdout)
        if profile == 'smoke':
            if kind != 'metadata' and recipe['expected'] != scale.expected(n):
                raise ValueError('independent Python record calibration')
            if recipe['label_digest'] != scale.expected_labels(recipe['labels']):
                raise ValueError('independent Python label calibration')
            calibration.append(dict(kind=kind, count=n, recipe=recipe))
        source_identity = writer_scale.hash_record(source)
        if kind == 'metadata':
            native_validation = writer_scale.measure([native, 'native-universe', source, 'metadata', n], area, f'native-{kind}-{n}', reference_metadata=True)
            if native_validation['result']['label_digest'] != recipe['label_digest'] or native_validation['result']['count'] != n:
                raise ValueError('native metadata identities mismatch')
            labels = call('labels', source, index, kind, n, stride, 'all', f'labels-{n}')
            if identity(phases(labels)[0]) != dict(count=n, digest=recipe['label_digest']):
                raise ValueError('complete bounded label stream mismatch')
            expected_all = dict(count=n, digest=native_validation['result']['digest'])
        else:
            native_validation = scale.measure([native, 'native', source, 'records', n], area, f'native-{kind}-{n}', timeout=7200)
            if native_validation['result']['all'] != recipe['expected']['all'] or native_validation['result']['label_digest'] != recipe['label_digest']:
                raise ValueError('native full records/metadata IDs mismatch')
            labels = None
            expected_all = recipe['expected']['all']
        case = dict(kind=kind, count=n, logical_bytes=n * (64 if kind == 'metadata' else 120), source=source_identity, recipe=recipe, native_validation=native_validation, labels=labels, stride=stride, trials=[])
        cases.append(case)
        progress()
        for trial in range(TRIALS):
            name = f'{kind}-{n}-{trial}'
            record = {}
            case['trials'].append(record)
            scale.evict(source)
            record['cold_serial'] = call('serial', source, index, kind, n, stride, 'leading', name + '-cold')
            progress()
            scale.evict(source)
            record['build'] = call('build', source, index, kind, n, stride, 'all', name + '-build')
            progress()
            build = phases(record['build'])[0]
            if not build['complete'] or not build['published'] or build['rows_decoded'] != n or (build['index_bytes'] > QUOTA):
                raise ValueError('incomplete/quota-invalid explicit build')
            with index.open('rb') as f:
                header = f.read(1024)
            if header[24:56].hex() != source_identity['sha256']:
                raise ValueError('independent source SHA-256 header mismatch')
            scale.evict(source)
            scale.evict(index)
            record['session'] = call('session', source, index, kind, n, stride, 'suite-directory' if kind == 'directory' else 'suite', name + '-session', 3)
            progress()
            scale.evict(source)
            record['serial_matrix'] = call('serial-matrix', source, index, kind, n, stride, 'directory' if kind == 'directory' else 'all', name + '-serial-matrix')
            progress()
            compare_queries(record['serial_matrix'], record['session'], record['cold_serial'], source_identity['bytes'])
            all_rows = next((r for r in phases(record['session'])[1:] if r['query'] == 'all'))
            if identity(all_rows) != expected_all:
                raise ValueError('full native/serial/indexed ordered digest mismatch')
            record['index'] = writer_scale.hash_record(index)
            if record['index']['bytes'] != build['index_bytes'] or phases(record['session'])[0]['index_bytes'] != build['index_bytes']:
                raise ValueError('sidecar extent mismatch')
            progress()
            index.unlink()
        if writer_scale.hash_record(source) != source_identity:
            raise ValueError('M5 mutated GDX source')
        if profile == 'scale':
            case['performance'] = performance(case)
        progress()
        source.unlink()
    if profile == 'scale':
        qualify(cases)
    if hashes != {p: oracle.file_digest(ROOT / p) for p in SOURCES}:
        raise ValueError('M5 sources changed during qualification')
    return dict(source_revision=revision, source_hashes=hashes, profile=profile, m5_qualified=profile == 'scale', process_cap=CAP, library_budget=BUDGET, index_quota=QUOTA, handle_limit=2, page_cache_limit=1024, trials=TRIALS, root=str(area.relative_to(ROOT)), cases=cases, calibration=calibration, oracle=reference, worker_sha256=oracle.file_digest(worker), native_binary_sha256=oracle.file_digest(native), compiler=run(['rustc', '--version', '--verbose'], capture=True).stdout, hardware=dict(platform=platform.platform(), cpu=next((l.split(':', 1)[1].strip() for l in Path('/proc/cpuinfo').read_text().splitlines() if l.startswith('model name')))), storage=run(['df', '-T', ROOT], capture=True).stdout, cache_policy='Cold phases use fresh process plus per-file fsync/POSIX_FADV_DONTNEED before each trial; advisory only, device caches uncontrolled. First query follows strict replay. Three warm queries reuse one validated session. Functional matrix follows warm queries; serial matrix uses a fresh process after advisory source eviction.', retention='Owned generated source/index files retired only after complete native validation, ordered serial/indexed comparison and SHA-256; compact reports/binaries retained', limitations=['Linux RLIMIT_AS/RSS containment only; native metadata reference uses explicit 16 GiB cap because upstream eagerly loads dictionaries', 'Directory itself must exceed 512 MiB, and its complete physical/logical sidecar input exceeds 4x cap. Records and metadata source inputs independently exceed 4x cap.', 'No privileged global/device cache eviction; cold means documented advisory eviction', 'Source I/O deltas may include concurrent same-source work; workers are single-threaded', 'No independent approval or automatic M5 release'])
