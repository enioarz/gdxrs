"""Development-only M2 streaming workloads, OS containment and measurements."""
import json
import os
from pathlib import Path
import platform
import resource
import shutil
import statistics
import subprocess
import sys
import time
import uuid

import oracle

ROOT = Path(__file__).resolve().parents[1]
CAP = 512 * 1024 * 1024
BUDGET = 256 * 1024 * 1024
SEED = 0x1640
MASK = (1 << 64) - 1
INITIAL = 14695981039346656037
SOURCES = ['tools/scale.py', 'tools/scale/driver.cpp', 'tools/scale/CMakeLists.txt',
           'examples/scale_sink.rs', 'Cargo.toml', 'Cargo.lock'] + sorted(str(p.relative_to(ROOT)) for p in (ROOT/'src').rglob('*.rs'))


def step(h, word):
    return ((((h << 7) | (h >> 57)) & MASK) ^ word) * 0x9e3779b185ebca87 & MASK


def label(n):
    return f'u{1_000_000_000+n}'.encode().ljust(63, b'x')


def expected(rows):
    """Small independent recipe; never reads generator/native/Rust output."""
    result = {mode: dict(count=0, digest=INITIAL) for mode in ['all', 'leading', 'nonleading', 'head']}
    for n in range(rows):
        words = [n // 1024 + 1] + [1] * 18 + [n % 1024 + 1]
        words += [0x3ff0000000000000 | ((n*17+f*12345+SEED) & ((1 << 52)-1)) for f in range(5)]
        modes = ['all']
        if 2 <= words[0] <= 3:
            modes.append('leading')
        if words[19] <= 3:
            modes.append('nonleading')
        if n < 13:
            modes.append('head')
        for mode in modes:
            item = result[mode]
            for word in words:
                item['digest'] = step(item['digest'], word)
            item['count'] += 1
    for item in result.values():
        item['digest'] = f"{item['digest']:016x}"
    return result


def expected_labels(labels):
    digest = INITIAL
    for n in range(1, labels+1):
        digest = step(digest, n)
        for byte in label(n):
            digest = step(digest, byte)
    return f'{digest:016x}'


def expected_lookup(labels):
    selected = labels // 4
    digest = INITIAL
    count = 0
    for n in range(1, selected+1):
        k = selected-n
        for duplicate in range(2 if k % 16 == 15 else 1):
            ordinal = k+k//16+duplicate
            digest = step(step(digest, ordinal), n)
            for byte in label(n):
                digest = step(digest, byte)
            count += 1
    digest = step(step(digest, selected+selected//16), 0)
    for byte in label(labels+1):
        digest = step(digest, byte)
    return dict(count=count+1, digest=f'{digest:016x}')


def summary(values):
    if len(values) < 3 or any(value <= 0 for value in values):
        raise ValueError('at least three positive observations required for dispersion')
    return dict(trials=len(values), median=statistics.median(values), minimum=min(values),
                maximum=max(values), stdev=statistics.stdev(values))


def child_limits():
    resource.setrlimit(resource.RLIMIT_AS, (CAP, CAP))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))


def measure(argv, area, name, *, timeout=1800):
    """One non-forking consumer: bounded stdout, wait4 RSS/CPU, sampled /proc."""
    output = area / f'{name}.stdout'
    stderr = area / f'{name}.stderr'
    started = time.monotonic()
    rss = vm = handles = samples = 0
    last_io = {}
    sampling_permission_denials = 0
    print('+ OS-capped', ' '.join(map(str, argv)), flush=True)
    with output.open('xb') as out, stderr.open('xb') as err:
        process = subprocess.Popen(list(map(str, argv)), cwd=ROOT, stdout=out, stderr=err,
                                   preexec_fn=child_limits)
        try:
            while True:
                pid, status, usage = os.wait4(process.pid, os.WNOHANG)
                if pid:
                    process.returncode = os.waitstatus_to_exitcode(status)
                    break
                try:
                    state = Path(f'/proc/{process.pid}/status').read_text()
                    fields = {line.split(':', 1)[0]: line.split(':', 1)[1].strip() for line in state.splitlines() if ':' in line}
                    rss = max(rss, int(fields.get('VmRSS', '0 kB').split()[0])*1024)
                    vm = max(vm, int(fields.get('VmSize', '0 kB').split()[0])*1024)
                    handles = max(handles, len(list(Path(f'/proc/{process.pid}/fd').iterdir())))
                    last_io = {line.split(':')[0]: int(line.split(':')[1]) for line in Path(f'/proc/{process.pid}/io').read_text().splitlines()}
                    samples += 1
                except PermissionError:
                    # Linux may revoke /proc access during exit/exec. wait4 RSS
                    # and the worker's enforced self-checked AS cap remain mandatory.
                    sampling_permission_denials += 1
                except (FileNotFoundError, ProcessLookupError):
                    pass
                if time.monotonic()-started > timeout:
                    process.kill()
                    raise TimeoutError(f'{name} exceeded {timeout}s')
                if output.stat().st_size > 1024*1024 or stderr.stat().st_size > 1024*1024:
                    process.kill()
                    raise ValueError('measurement output exceeded fixed one-MiB allowance')
                time.sleep(.05)
        finally:
            if process.returncode is None:
                process.kill()
                _, status, _ = os.wait4(process.pid, 0)
                process.returncode = os.waitstatus_to_exitcode(status)
    record = dict(argv=list(map(str, argv)), exit_code=process.returncode,
                  wall_seconds=time.monotonic()-started, cpu_user_seconds=usage.ru_utime,
                  cpu_system_seconds=usage.ru_stime, peak_rss_bytes=usage.ru_maxrss*1024,
                  sampled_peak_rss_bytes=rss, sampled_peak_virtual_bytes=vm,
                  sampled_peak_fds=handles, proc_samples=samples, proc_sampling_permission_denials=sampling_permission_denials, last_sampled_io=last_io,
                  os_cap=dict(kind='RLIMIT_AS', soft_bytes=CAP, hard_bytes=CAP),
                  stdout=str(output.relative_to(ROOT)), stderr=str(stderr.relative_to(ROOT)))
    (area/f'{name}.measurement.json').write_text(json.dumps(record, indent=2)+'\n')
    if process.returncode:
        raise ValueError(f'{name} failed ({process.returncode}): {stderr.read_text()[:4096]}')
    if not 0 < record['peak_rss_bytes'] <= CAP or vm > CAP:
        raise ValueError('missing/excessive process RSS or address space')
    record['result'] = json.loads(output.read_text())
    return record


def validate_scan(trial, recipe, mode):
    actual = trial['result']
    target = dict(count=0, digest=f'{INITIAL:016x}') if mode == 'zero' else recipe['expected'][mode]
    if any(actual[key] != target[key] for key in ['count', 'digest']):
        raise ValueError('record count/ordered digest mismatch')
    decoded = actual['decoded_rows']
    expected_rows = recipe['count'] if mode in ['all', 'leading', 'nonleading'] else min(recipe['count'], 13) if mode == 'head' else 0
    if decoded != expected_rows:
        raise ValueError('unexpected decoded work, or accepted-row limit violated')
    if actual['count'] and mode in ['all', 'head'] and actual['first_bytes'] > 512*1024:
        raise ValueError('first batch read too much source data')
    if actual['root_peak'] > BUDGET:
        raise ValueError('tracked root quota exceeded')


def check_scale(recipe, kind):
    if recipe['bytes'] < 4*CAP or recipe['logical_output_bytes'] < 4*CAP:
        raise ValueError(f'{kind} input/output does not exceed four times the OS cap')
    if kind == 'metadata' and recipe['labels']*64 < 4*CAP:
        raise ValueError('independently oversized metadata required')


def evict(path):
    # Advisory per-file eviction, without global caches or privileged operations.
    with path.open('rb') as stream:
        os.fsync(stream.fileno())
        os.posix_fadvise(stream.fileno(), 0, 0, os.POSIX_FADV_DONTNEED)


def build(run, area):
    oracle.build(run)
    oracle.executable()
    reference = json.loads((oracle.AREA/'current.json').read_text())
    library = Path(reference['executable']).parent.parent/'gdx/libgdx-static.a'
    directory = area/'native-build'
    run([oracle.cmake(), '-S', ROOT/'tools/scale', '-B', directory, '-DCMAKE_BUILD_TYPE=Release',
         f'-DGDX_ROOT={oracle.AREA/"gdx"}', f'-DGDX_LIBRARY={library}'])
    run([oracle.cmake(), '--build', directory, '--parallel', '2'])
    binaries = {}
    for name, features in [('default', []), ('minimal', ['--no-default-features'])]:
        run(['cargo', 'build', '--locked', '--release', '--example', 'scale_sink', *features])
        destination = area/f'sink-{name}'
        shutil.copyfile(ROOT/'target/release/examples/scale_sink', destination)
        destination.chmod(0o700)
        binaries[name] = destination
    return directory/'gdxrs-scale', binaries, reference


def check(run, profile):
    if sys.platform != 'linux' or not hasattr(os, 'wait4') or not hasattr(os, 'posix_fadvise'):
        raise ValueError('Linux containment/measurement unavailable; scale gate not qualified')
    if profile not in ['smoke', 'scale']:
        raise ValueError('profile must be smoke or scale')
    hashes = {name: oracle.file_digest(ROOT/name) for name in SOURCES}
    revision = subprocess.check_output(['git','rev-parse','HEAD'], cwd=ROOT,text=True).strip()
    dirty = subprocess.check_output(['git','status','--porcelain','--',*SOURCES], cwd=ROOT,text=True).strip()
    if profile == 'scale' and dirty:
        raise ValueError('commit measured source changes before scale qualification')
    area = ROOT/'.work/scale'/uuid.uuid4().hex
    area.mkdir(parents=True)
    regressions = {}
    if profile == 'scale':
        for command in ['reader-check', 'm0-check', 'label-pages-check']:
            run([sys.executable, ROOT/'tools/dev.py', command], timeout=900)
            report = max((ROOT/'.work/reports').glob(command+'-*.json'), key=lambda p: p.stat().st_mtime_ns)
            data = json.loads(report.read_text())
            if data['revision'] != revision or data['status'] != 'passed' or data['exit_code'] != 0:
                raise ValueError('fresh regression gate did not pass at measured revision')
            regressions[command] = dict(artifact=str(report.relative_to(ROOT)), run_id=data['run_id'], revision=revision, result=data['result'])
    native, binaries, reference = build(run, area)
    probe = measure([sys.executable, '-c',
                     'import resource,json; assert resource.getrlimit(resource.RLIMIT_AS)==(536870912,536870912); '
                     'x=bytearray(16*1024*1024);\ntry: x=bytearray(536870913)\nexcept MemoryError: print(json.dumps({"allocation_denied":True}))\nelse: raise RuntimeError("cap not enforced")'], area, 'cap-probe')
    if probe['result'] != dict(allocation_denied=True):
        raise ValueError('OS cap negative control failed')
    cases = []
    # Fast native/Python/Rust agreement precedes every large generation.
    sizes = [('records', 4096), ('metadata', 4096)]
    if profile == 'scale':
        if shutil.disk_usage(area).free < 18*1024**3:
            raise ValueError('scale qualification needs at least 18 GiB free scratch')
        sizes += [('records', 10_000_000), ('records', 20_000_000),
                  ('metadata', 18_000_000), ('metadata', 36_000_000)]
    for kind, count in sizes:
        name = f'{kind}-{count}'
        path = area/f'{name}.gdx'
        generated = measure([native, 'generate', path, kind, count], area, f'{name}-generate')
        recipe = generated['result']
        if recipe['bytes'] != path.stat().st_size:
            raise ValueError('generator file size mismatch')
        recipe['sha256'] = oracle.file_digest(path)
        if count == 4096:
            if kind == 'records' and recipe['expected'] != expected(count):
                raise ValueError('generator differs from Python independent record recipe')
            if recipe['label_digest'] != expected_labels(recipe['labels']):
                raise ValueError('generator differs from Python independent labels')
            if dict(count=recipe['lookup_count'], digest=recipe['lookup_digest']) != expected_lookup(recipe['labels']):
                raise ValueError('generator differs from Python independent request results')
        if profile == 'scale' and count in [20_000_000, 36_000_000]:
            check_scale(recipe, kind)
        case = dict(name=name, artifact=str(path.relative_to(ROOT)), recipe=recipe,
                    generation=generated, cold_cache='fresh process; advisory POSIX_FADV_DONTNEED before trial 0', trials=[])
        cases.append(case)
        modes = ['all', 'head', 'zero', 'leading', 'nonleading', 'domains'] if kind == 'records' else ['metadata', 'pages']
        if kind == 'metadata' and count in [4096, 36_000_000]:
            modes += ['lookup', 'cancel']
        if kind == 'metadata' and count in [4096, 18_000_000]:
            modes += ['membership']
        for mode in modes:
            repetitions = 3 if profile == 'scale' and count in [20_000_000, 36_000_000] and mode in ['all', 'metadata'] else 1
            for repeat in range(repetitions):
                if repeat == 0:
                    evict(path)
                scratch = area/f'{name}-{mode}-{repeat}-scratch'
                scratch.mkdir()
                trial = measure([binaries['default'], mode, path, count, scratch], area, f'{name}-{mode}-{repeat}')
                trial.update(mode=mode, repeat=repeat, cache='advisory eviction' if repeat == 0 else 'OS cache reused; library cache empty')
                if kind == 'records' and mode in ['all', 'head', 'zero', 'leading', 'nonleading']:
                    validate_scan(trial, recipe, mode)
                if mode == 'metadata' and (trial['result']['count'] != count or trial['result']['digest'] != recipe['label_digest']):
                    raise ValueError('metadata count/digest mismatch')
                if mode == 'lookup' and any(trial['result'][key] != recipe['lookup_'+key] for key in ['count', 'digest']):
                    raise ValueError('lookup count/digest mismatch')
                if list(scratch.iterdir()):
                    raise ValueError('scratch directory leaked after success/cancel')
                scratch.rmdir()
                case['trials'].append(trial)
        if kind == 'records':
            for repeat in range(3 if profile == 'scale' and count == 20_000_000 else 1):
                if repeat == 0:
                    evict(path)
                trial = measure([native, 'native', path, kind, count], area, f'{name}-native-{repeat}')
                if trial['result']['all'] != recipe['expected']['all']:
                    raise ValueError('native digest/count differs')
                trial.update(mode='native-all', repeat=repeat)
                case['trials'].append(trial)
        if count == 4096 and kind == 'metadata':
            trial = measure([native, 'native-labels', path, kind, count], area, f'{name}-native-labels')
            if trial['result'] != dict(count=count, digest=recipe['label_digest']):
                raise ValueError('native metadata digest/count differs')
            trial.update(mode='native-metadata')
            case['trials'].append(trial)
        if count == 4096:
            mode = 'all' if kind == 'records' else 'metadata'
            scratch = area/f'{name}-minimal-scratch'
            scratch.mkdir()
            trial = measure([binaries['minimal'], mode, path, count, scratch], area, f'{name}-minimal')
            if mode == 'all':
                validate_scan(trial, recipe, mode)
            elif trial['result']['digest'] != recipe['label_digest']:
                raise ValueError('minimal metadata mismatch')
            trial.update(mode='minimal-'+mode)
            case['trials'].append(trial)
            scratch.rmdir()
        for mode in ['all', 'native-all', 'metadata']:
            samples = [t for t in case['trials'] if t['mode'] == mode]
            if len(samples) >= 3:
                case.setdefault('summaries', {})[mode] = {field: summary([t[field] for t in samples]) for field in ['wall_seconds', 'cpu_user_seconds', 'peak_rss_bytes']}
        (area/'cases.json').write_text(json.dumps(cases, indent=2)+'\n')
    growth = []
    if profile == 'scale':
        for kind, lower, upper, mode in [('records',10_000_000,20_000_000,'all'),('metadata',18_000_000,36_000_000,'metadata')]:
            a = next(c for c in cases if c['name'] == f'{kind}-{lower}')
            b = next(c for c in cases if c['name'] == f'{kind}-{upper}')
            small = next(t for t in a['trials'] if t['mode'] == mode)
            large = max((t for t in b['trials'] if t['mode'] == mode), key=lambda t:t['peak_rss_bytes'])
            if large['peak_rss_bytes'] > small['peak_rss_bytes'] + 32*1024*1024:
                raise ValueError('doubling input increased RSS beyond predeclared 32 MiB headroom')
            if large['result']['root_peak'] > small['result']['root_peak'] + 1024*1024:
                raise ValueError('doubling input increased tracked root by over one MiB')
            growth.append(dict(kind=kind, lower_count=lower, upper_count=upper, lower_rss=small['peak_rss_bytes'], upper_rss=large['peak_rss_bytes'], lower_root=small['result']['root_peak'], upper_root=large['result']['root_peak']))
    if hashes != {name: oracle.file_digest(ROOT/name) for name in SOURCES} or revision != subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip():
        raise ValueError('measured source changed during qualification')
    return dict(profile=profile, m2_scale_qualified=profile == 'scale', seed=SEED, regressions=regressions, growth=growth, measured_source_dirty=bool(dirty),
                root_budget_bytes=BUDGET, os_cap_bytes=CAP,
                artifact=str(area.relative_to(ROOT)), cap_negative_control=probe,
                source_revision=revision, source_hashes=hashes,
                compiler=run(['rustc','--version'],capture=True).stdout.strip(),
                native_compiler=run(['g++','--version'],capture=True).stdout.splitlines()[0],
                hardware=dict(platform=platform.platform(), cpu=next((line.split(':',1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')), 'unknown'), cpus=os.cpu_count()),
                oracle=reference, cases=cases,
                unavailable=['native Windows/macOS/big-endian hardware', 'physical device cache eviction', 'cgroup process-tree/RSS containment'],
                native_metadata_comparison='small metadata independently checked; huge native dictionary loading not attempted',
                limitations=['One-off functional modes have no dispersion estimate', 'OS page cache may remain warm after advisory eviction', 'Sampled /proc I/O is a lower-bound snapshot; Reader counters cover complete source work', 'RSS excludes kernel page cache/filesystem blocks', 'RLIMIT_AS contains each non-forking consumer, not a descendant process tree'])
