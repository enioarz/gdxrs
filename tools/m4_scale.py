"""M4 acceptance. Linux-only capped workers; independent complete native sinks.

Smoke exercises the recipes and validators but cannot qualify oversized inputs.
The native timing reference is an explicit eager composite, not hidden native
string/mapped API internals; see docs/m4-api.md for comparison limitations.
"""
import functools
import json
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys
import uuid
import m4_api
import oracle
import scale
import writer_scale

ROOT = Path(__file__).resolve().parents[1]
CAP = 512 * 1024**2
BUDGET = 256 * 1024**2
KINDS = ('unsorted', 'strings', 'mapped', 'dictionary', 'errors', 'append')
PHASES = ('registration_seconds', 'remapping_seconds', 'ingestion_seconds',
          'merge_seconds', 'replay_encoding_seconds', 'copy_seconds', 'finalization_seconds')
INITIAL = 0xcbf29ce484222325
MASK = (1 << 64) - 1
SOURCES = sorted(['tools/m4_scale.py', 'tools/m4_api.py', 'docs/m4-api.csv',
                  'tools/dev.py', 'tools/scale.py', 'tools/writer_scale.py',
                  'docs/api-parity.csv', 'docs/api-properties.csv',
                  'tools/scale/driver.cpp', 'examples/m4_worker.rs',
                  'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'] +
                 [str(p.relative_to(ROOT)) for p in (ROOT/'tools/m4').iterdir() if p.is_file()] +
                 [str(p.relative_to(ROOT)) for p in (ROOT/'src').rglob('*.rs')])


def step(h, word):
    rotated = ((h << 7) | (h >> 57)) & MASK
    return ((rotated ^ word) * 0x9e3779b185ebca87) & MASK


def fold(h, words):
    for word in words:
        h = step(h, word)
    return h


def keys(i, dim):
    return [i//65536+1, i % 65536+1][:dim] + [((i*(2*d+1)+0x1640) & 65535)+1 for d in range(2, dim)]


def bits(i, fields):
    return [0x3ff0000000000000 | ((i*17+f*12345+0x1640) & ((1 << 52)-1)) for f in range(fields)]


@functools.lru_cache(maxsize=8)
def label_digest(count, short):
    h = INITIAL
    for i in range(1, count+1):
        text = f'u{1_000_000_000+i}'.encode()
        if not short:
            text = text.ljust(63, b'x')
        h = fold(step(h, i), text)
    return f'{h:016x}'


def python_expected(kind, n):
    """Only small recipes. Diagnostics use independently enumerated input order."""
    if not 8 <= n <= 16384:
        raise ValueError('Python calibration is explicitly small')
    dim, fields = (1, 1) if kind == 'dictionary' else (2 if kind == 'strings' else 20, 5)
    indices = [1, 2, n//2, n-1, n] if kind == 'dictionary' else range(n)
    h = INITIAL
    for i in indices:
        h = fold(h, ([i] if kind == 'dictionary' else keys(i, dim)) + bits(i, fields))
    diagnostics = []
    if kind == 'errors':
        ordinal = 0
        for i in reversed(range(n)):
            ordinal += 1  # valid row
            if i % 2 == 0:
                ordinal += 1
                k = keys(i, 20)
                k[0] = 65537
                diagnostics.append((k, ordinal, 1, bits(i, 5)))
            if i % 3 == 0:
                ordinal += 1
                diagnostics.append((keys(i, 20), ordinal, 2, bits(i, 5)))
        diagnostics.sort(key=lambda r: (r[0], r[1]))
    eh = INITIAL
    for k, ordinal, tag, values in diagnostics:
        eh = fold(eh, [tag, ordinal, int(tag == 1)] + k + values)
    labels = n if kind == 'dictionary' else 65536 + int(kind == 'errors')
    return dict(count=5 if kind == 'dictionary' else n, digest=f'{h:016x}',
                labels=labels, label_digest=label_digest(labels, kind == 'strings'),
                errors=len(diagnostics), error_bytes=len(diagnostics)*159, error_digest=f'{eh:016x}')


def validate_worker(trial, expected, kind):
    r = trial['result']
    for name in ['count', 'labels', 'errors', 'error_bytes', 'error_digest']:
        if r[name] != expected[name]:
            raise ValueError(f'M4 worker {name} mismatch')
    if not r['os_cap_verified'] or r['root_limit'] != BUDGET or r['root_peak'] > BUDGET:
        raise ValueError('M4 shared root/OS self-check failed')
    if trial['os_cap']['hard_bytes'] != CAP or trial['os_cap']['soft_bytes'] != CAP or trial['peak_rss_bytes'] > CAP:
        raise ValueError('M4 enforced process containment missing')
    if r['shared_root'] != (kind == 'append'):
        raise ValueError('append must use one shared root')
    if r['scratch_peak'] > 32*1024**3 or r['scratch_peak_handles'] > (14 if kind == 'append' else 12):
        raise ValueError('M4 scratch quota/handles exceeded')
    if any(r[p] < 0 for p in PHASES):
        raise ValueError('negative phase duration')


def qualify(cases):
    if {c['kind'] for c in cases} != set(KINDS):
        raise ValueError('all six independently oversized families required')
    for kind in KINDS:
        family = sorted((c for c in cases if c['kind'] == kind), key=lambda c: c['count'])
        writer_scale.growth(family)
        last = family[-1]
        if min(last['bytes'], last['logical_bytes']) <= 4*CAP:
            raise ValueError(f'{kind} physical AND logical workload must exceed four times cap')
        if kind == 'dictionary' and last['labels']*64 <= 4*CAP:
            raise ValueError('dictionary must be independently oversized')
        if kind == 'errors' and last['worker']['result']['error_bytes'] <= 4*CAP:
            raise ValueError('diagnostic stream must be independently oversized')


def phase_summary(values):
    if len(values) < 3 or any(value < 0 for value in values):
        raise ValueError('three nonnegative phase observations required')
    return dict(trials=len(values), median=statistics.median(values), minimum=min(values),
                maximum=max(values), stdev=statistics.stdev(values))


def measure(argv, area, name, reference=False):
    old = scale.CAP
    try:
        scale.CAP = writer_scale.NATIVE_METADATA_CAP if reference else CAP
        return scale.measure(argv, area, name, timeout=24*3600)
    finally:
        scale.CAP = old


def build(run, area):
    oracle.build(run)
    ref = json.loads((ROOT/'.work/oracle/current.json').read_text())
    library = Path(ref['executable']).parents[1]/'gdx/libgdx-static.a'
    native = area/'native-build'
    run([oracle.cmake(), '-S', ROOT/'tools/m4', '-B', native, '-DCMAKE_BUILD_TYPE=Release',
         f'-DGDX_ROOT={ROOT/".work/oracle/gdx"}', f'-DGDX_LIBRARY={library}'])
    run([oracle.cmake(), '--build', native, '--parallel', '2'])
    run(['cargo', 'build', '--locked', '--release', '--example', 'm4_worker'])
    rust = area/'m4_worker'
    shutil.copyfile(ROOT/'target/release/examples/m4_worker', rust)
    rust.chmod(0o700)
    return native/'gdxrs-m4', rust, ref


def check(run, profile):
    if sys.platform != 'linux' or profile not in ['smoke', 'scale']:
        raise ValueError('M4 acceptance requires Linux and smoke/scale profile')
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    hashes = {p: oracle.file_digest(ROOT/p) for p in SOURCES}
    if profile == 'scale' and subprocess.check_output(['git', 'status', '--porcelain', '--', *SOURCES], text=True).strip():
        raise ValueError('commit measured sources before scale qualification')
    api = m4_api.check()
    area = ROOT/'.work/m4-acceptance'/uuid.uuid4().hex
    area.mkdir(parents=True)
    native, rust, ref = build(run, area)
    benchmarks, cases, calibration = [], [], []

    def progress():
        (area/'progress.json').write_text(json.dumps(dict(source_revision=revision, source_hashes=hashes,
            calibration=calibration, benchmarks=benchmarks, cases=cases, m4_scale_qualified=False), indent=2)+'\n')

    def expected(kind, n):
        # Runs n rows in native tooling, not a closed-form substitute for validation.
        return json.loads(run([native, 'expected', kind, n], capture=True, timeout=1800).stdout)

    def verify(path, kind, n, want, scratch, name):
        a = measure([native, 'verify', path, kind, n], area, name+'-native', reference=kind == 'dictionary')
        b = measure([rust, 'validate', kind, path, scratch, n, 'raw'], area, name+'-rust')
        for trial in (a, b):
            for field in ['count', 'digest', 'label_digest']:
                if trial['result'][field] != want[field]:
                    raise ValueError('independent complete ordered record/UEL ID validation mismatch')
        if (not b['result']['os_cap_verified'] or b['result']['root_peak'] > BUDGET
                or b['result']['root_limit'] != BUDGET or b['os_cap']['hard_bytes'] != CAP
                or b['os_cap']['soft_bytes'] != CAP or b['peak_rss_bytes'] > CAP):
            raise ValueError('readback containment mismatch')
        return a, b

    # Independent small Python enumeration calibrates every native recipe,
    # including diagnostic order and original input ordinals, before scale use.
    for kind in KINDS:
        n = 1024 if kind == 'dictionary' else 8192
        want = expected(kind, n)
        if want != python_expected(kind, n):
            raise ValueError('Python/native recipe calibration mismatch: '+kind)
        calibration.append(dict(kind=kind, count=n, expected=want))
    progress()

    # At least three alternating fresh-output trials; every output is read by
    # BOTH independent implementations before its hash is recorded and retired.
    for kind, compression in [(k, 'raw') for k in KINDS]+[('unsorted', 'zlib'), ('strings', 'zlib')]:
        n = (250_000 if kind == 'dictionary' else 1_000_000) if profile == 'scale' else (1024 if kind == 'dictionary' else 8192)
        want = expected(kind, n)
        source = area/f'bench-input-{kind}-{compression}.gdx'
        if kind == 'append':
            measure([native, 'generate', source, n], area, 'bench-input')
        source_identity = writer_scale.hash_record(source) if source.exists() else None
        trials = {p: [] for p in ['rust', 'native']}
        for i in range(3):
            for producer in (['native', 'rust'] if i % 2 else ['rust', 'native']):
                name = f'bench-{kind}-{compression}-{producer}-{i}'
                path, scratch = area/(name+'.gdx'), area/(name+'-scratch')
                scratch.mkdir()
                argv = [rust, 'write', kind, path, scratch, n, compression] if producer == 'rust' else [native, 'write', path, kind, n, compression]
                if kind == 'append':
                    argv.append(source)
                trial = measure(argv, area, name, reference=producer == 'native')
                if producer == 'rust':
                    validate_worker(trial, want, kind)
                elif any(trial['result'][f] != want[f] for f in ['errors', 'error_digest']):
                    raise ValueError('native composite diagnostics mismatch')
                a, b = verify(path, kind, n, want, scratch, name)
                trial.update(output=writer_scale.hash_record(path), native_validation=a, rust_validation=b)
                trials[producer].append(trial)
                path.unlink()
                scratch.rmdir()
        if source_identity is not None:
            if writer_scale.hash_record(source) != source_identity:
                raise ValueError('append mutated input')
            source.unlink()
        phases = {p: {phase: phase_summary([t['result'][phase] for t in rows]) for phase in PHASES} for p, rows in trials.items()}
        benchmarks.append(dict(kind=kind, compression=compression, count=n, trials=trials, phases=phases, input=source_identity))
        progress()

    for kind in KINDS:
        counts = ([9_000_000, 34_000_000] if kind == 'dictionary' else [13_000_000, 52_000_000] if kind == 'strings' else [4_500_000, 18_000_000]) if profile == 'scale' else [4096, 16384]
        for n in counts:
            if profile == 'scale' and shutil.disk_usage(area).free < 20*1024**3:
                raise ValueError('insufficient scratch headroom; retain thresholds and retire only proven regenerable artifacts')
            name = f'{kind}-{n}'
            output, scratch, source = area/(name+'.gdx'), area/(name+'-scratch'), area/(name+'-input.gdx')
            scratch.mkdir()
            want = expected(kind, n)
            if kind == 'append':
                measure([native, 'generate', source, n], area, name+'-input')
                # Qualifies the independently generated source before importing.
                verify(source, 'unsorted', n, want, scratch, name+'-input-verify')
            identity = writer_scale.hash_record(source) if source.exists() else None
            argv = [rust, 'write', kind, output, scratch, n, 'raw']+([source] if kind == 'append' else [])
            trial = measure(argv, area, name)
            validate_worker(trial, want, kind)
            a, b = verify(output, kind, n, want, scratch, name)
            if identity is not None and writer_scale.hash_record(source) != identity:
                raise ValueError('append source identity changed')
            logical = n * (64 if kind == 'dictionary' else 48 if kind == 'strings' else 120)
            cases.append(dict(kind=kind, count=n, labels=want['labels'], logical_bytes=logical, worker=trial,
                native_validation=a, rust_validation=b, input=identity, **writer_scale.hash_record(output)))
            progress()
            output.unlink()
            scratch.rmdir()
            if source.exists():
                source.unlink()
    if profile == 'scale':
        qualify(cases)
    if hashes != {p: oracle.file_digest(ROOT/p) for p in SOURCES}:
        raise ValueError('sources changed during acceptance')
    return dict(profile=profile, m4_scale_qualified=profile == 'scale', source_revision=revision, source_hashes=hashes,
        root=str(area.relative_to(ROOT)), api=api, calibration=calibration, benchmarks=benchmarks, cases=cases,
        native_oracle=ref, binary_sha256=dict(rust=oracle.file_digest(rust), native=oracle.file_digest(native)),
        compiler=run(['rustc', '--version', '--verbose'], capture=True).stdout,
        native_compiler=run(['c++', '--version'], capture=True).stdout,
        hardware=dict(platform=platform.platform(), cpu=next(l.split(':', 1)[1].strip() for l in Path('/proc/cpuinfo').read_text().splitlines() if l.startswith('model name'))),
        storage=run(['df', '-T', ROOT], capture=True).stdout,
        library_budget=BUDGET, writer_process_cap=CAP, native_reference_cap=writer_scale.NATIVE_METADATA_CAP,
        cache_policy='Warm OS cache; fresh outputs; optional root-charged 64 MiB per-store lookup ceiling; 32 MiB sort runs at >=1M inputs',
        retention='Generated artifacts removed only after full native/Rust validation and SHA-256; reports and failed artifacts retained',
        reference='Native raw API plus explicit eager C++ vector/std::sort/sparse unordered_map composite, not native legacy internal phase timings',
        compression='Rust miniz level 1; native default zlib level 6',
        durability='Rust sync_all; native gdxClose gives no equivalent durability promise')
