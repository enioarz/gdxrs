"""Reproducible small upstream baseline with independent ordered-result validation."""

import functools
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import struct
import subprocess
import time
import uuid

import oracle

ROOT = Path(__file__).resolve().parents[1]
MEASURED_FILES = ['tools/benchmark.py','tools/bench/driver.cpp','tools/bench/CMakeLists.txt','tools/oracle/sources.json']
WORKLOADS = {
    'parameter-uncompressed':dict(rows=1000000,labels=1000,symbols=1,dimension=2,fields=1,compression=0),
    'parameter-compressed':dict(rows=1000000,labels=1000,symbols=1,dimension=2,fields=1,compression=1),
    'variable':dict(rows=100000,labels=1000,symbols=1,dimension=2,fields=5,compression=0),
    'metadata':dict(rows=2000,labels=100000,symbols=2000,dimension=0,fields=1,compression=0),
}


def fingerprint(revision=None):
    if revision is None:
        return {name:oracle.file_digest(ROOT/name) for name in MEASURED_FILES}
    result = {}
    for name in MEASURED_FILES:
        obj = subprocess.run(['git','show',f'{revision}:{name}'],cwd=ROOT,capture_output=True)
        if obj.returncode:
            raise ValueError('measured revision lacks benchmark source: '+name)
        result[name] = hashlib.sha256(obj.stdout).hexdigest()
    return result


def current_baseline():
    pointer = ROOT/'.work/bench/current.json'
    if not pointer.is_file():
        raise ValueError('no current baseline; run benchmark-oracle --profile small or pass --baseline')
    path = (ROOT/json.loads(pointer.read_text())['artifact']).resolve()
    if not path.is_relative_to((ROOT/'.work/bench/runs').resolve()):
        raise ValueError('current baseline path outside benchmark runs')
    return path


@functools.lru_cache(maxsize=8)
def expected_digest(rows,dimension,fields):
    """Input recipe only: no native output, GDX bytes or Rust decoder is read."""
    digest = 14695981039346656037
    mask = (1<<64)-1
    for ordinal in range(rows):
        keys = struct.pack('<II',ordinal//1000+1,ordinal%1000+1) if dimension else b''
        values = [ordinal+.25] if fields==1 else [ordinal+.25,-ordinal-.5,-100.25,100.75,1.]
        record = keys + struct.pack('<'+'d'*fields,*values)
        for byte in record:
            digest = ((digest ^ byte)*1099511628211)&mask
    return f'{digest:016x}'


def summary(values):
    if len(values)!=5 or any(not isinstance(v,(int,float)) or v<0 for v in values):
        raise ValueError('five nonnegative measured repetitions required')
    return dict(median=statistics.median(values),minimum=min(values),maximum=max(values),
                stdev=statistics.stdev(values))


def validate_scan(result,workload):
    expected = expected_digest(workload['rows'],workload['dimension'],workload['fields'])
    if result['schema']!=1 or result['rows']!=workload['rows'] or result['digest']!=expected:
        raise ValueError('benchmark ordered field digest/count mismatch')
    if result['scan_seconds']<=0 or result['peak_rss_bytes']<=0:
        raise ValueError('benchmark scan/RSS measurements missing')


def build(run):
    reference = json.loads((oracle.AREA/'current.json').read_text())
    oracle.executable() # checks pinned source content and executable identity
    library = Path(reference['executable']).parent.parent/'gdx/libgdx-static.a'
    library_hash = oracle.file_digest(library)
    compiler = run(['g++','--version'],capture=True).stdout.splitlines()[0]
    cmake = run([oracle.cmake(),'--version'],capture=True).stdout.splitlines()[0]
    identity = hashlib.sha256(json.dumps([fingerprint(),library_hash,compiler,cmake],sort_keys=True).encode()).hexdigest()
    directory = ROOT/'.work/bench/builds'/identity
    run([oracle.cmake(),'-S',ROOT/'tools/bench','-B',directory,'-DCMAKE_BUILD_TYPE=Release',
         '-DCMAKE_CXX_COMPILER=g++',f'-DGDX_ROOT={oracle.AREA / "gdx"}',f'-DGDX_LIBRARY={library}'])
    run([oracle.cmake(),'--build',directory,'--parallel','2'],timeout=180)
    return directory/'gdxrs-bench',dict(identity=identity,library_sha256=library_hash,compiler=compiler,cmake=cmake)


def execute(run,profile):
    if profile!='small': raise ValueError('only the small profile is executable in M0; see the documented large-profile recipe')
    exe,build_identity = build(run)
    directory = ROOT/'.work/bench/runs'/uuid.uuid4().hex;directory.mkdir(parents=True)
    revision = run(['git','rev-parse','HEAD'],capture=True).stdout.strip()
    if fingerprint(revision) != fingerprint():
        raise ValueError('commit benchmark source changes before measuring')
    results = []
    started=time.monotonic()
    for name,workload in WORKLOADS.items():
        # Establish expected output from the deterministic recipe before generation.
        wanted = expected_digest(workload['rows'],workload['dimension'],workload['fields'])
        def invoke(mode,path):
            result=json.loads(run([exe,mode,path,name],capture=True,timeout=180).stdout)
            if result['rows']!=workload['rows'] or result['schema']!=1: raise ValueError('benchmark write schema/count')
            if mode=='scan': validate_scan(result,workload)
            return result
        source = directory/(name+'.gdx')
        generation = invoke('write',source)
        warmup = invoke('scan',source)
        reads=[];writes=[];write_validations=[]
        for repetition in range(5):
            reads.append(invoke('scan',source))
            output=directory/f'{name}-write-{repetition}.gdx'
            writes.append(invoke('write',output))
            write_validations.append(invoke('scan',output))
            output.unlink() # owned generated output, validated before cleanup
        size=source.stat().st_size
        results.append(dict(name=name,workload=workload,input_path=str(source.relative_to(ROOT)),
                            input_sha256=oracle.file_digest(source),file_bytes=size,expected_digest=wanted,
                            generation=generation,warmup=warmup,reads=reads,writes=writes,
                            write_validations=write_validations,
                            scan_seconds=summary([r['scan_seconds'] for r in reads]),
                            open_seconds=summary([r['open_seconds'] for r in reads]),
                            metadata_seconds=summary([r['metadata_seconds'] for r in reads]),
                            write_seconds=summary([w['elapsed_seconds'] for w in writes]),
                            records_per_second=workload['rows']/statistics.median(r['scan_seconds'] for r in reads),
                            file_mib_per_second=size/(1024*1024)/statistics.median(r['scan_seconds'] for r in reads),
                            nominal_decoded_bytes=workload['rows']*(workload['dimension']*4+workload['fields']*8),
                            decoded_mib_per_second=workload['rows']*(workload['dimension']*4+workload['fields']*8)/(1024*1024)/statistics.median(r['scan_seconds'] for r in reads)))
    model='unavailable'
    cpuinfo=Path('/proc/cpuinfo')
    if cpuinfo.exists():
        model=next((line.split(':',1)[1].strip() for line in cpuinfo.read_text().splitlines() if line.startswith('model name')),model)
    report=dict(schema=1,profile='small',revision=revision,implementation_files=fingerprint(),
                build=build_identity,environment=dict(platform=platform.platform(),machine=platform.machine(),
                python=platform.python_version(),cpu_model=model,available_cpus=os.cpu_count()),
                cache_policy='fresh child process per invocation; OS cache naturally warm; no cold-cache claim; no index',
                digest_algorithm='FNV-1a-64 over ordered raw keys (u32 LE) then all returned value bits (u64 LE)',
                peak_rss_method='getrusage(RUSAGE_SELF) high-water mark; includes child launch/runtime overhead, not exclusive library allocation',
                warmups_per_scenario=1,measured_repetitions=5,cases=results,
                elapsed_including_validation_seconds=time.monotonic()-started,
                large_profile='unrun',rust_performance='unimplemented')
    report_path=directory/'baseline.json';report_path.write_text(json.dumps(report,indent=2)+'\n')
    validate_baseline(report_path)
    pointer=ROOT/'.work/bench/current.json'
    temporary=directory/'current.json'
    temporary.write_text(json.dumps(dict(artifact=str(report_path.relative_to(ROOT)))))
    temporary.replace(pointer)
    print('Baseline artifact:',report_path.relative_to(ROOT),flush=True)
    return dict(artifact=str(report_path.relative_to(ROOT)),scenarios=len(results),
                measured_scans=20,measured_writes=20,write_validation_scans=20,large_profile='unrun')


def validate_baseline(path):
    report=json.loads(Path(path).read_text())
    if report.get('schema')!=1 or report.get('profile')!='small' or report.get('implementation_files')!=fingerprint():
        raise ValueError('missing/stale benchmark source identity')
    if report['measured_repetitions']!=5 or report['warmups_per_scenario']!=1:
        raise ValueError('benchmark repetition policy mismatch')
    names=[case['name'] for case in report['cases']]
    if len(names)!=len(set(names)) or set(names)!=set(WORKLOADS): raise ValueError('missing/duplicate benchmark scenarios')
    subprocess.run(['git','merge-base','--is-ancestor',report['revision'],'HEAD'],cwd=ROOT,check=True)
    if fingerprint(report['revision']) != report['implementation_files']:
        raise ValueError('measured revision benchmark source identity mismatch')
    for case in report['cases']:
        workload=WORKLOADS[case['name']]
        if case['workload']!=workload: raise ValueError('benchmark workload changed')
        if case['expected_digest']!=expected_digest(workload['rows'],workload['dimension'],workload['fields']):
            raise ValueError('benchmark reference digest changed')
        path=(ROOT/case['input_path']).resolve()
        if not path.is_relative_to(ROOT/'.work/bench/runs') or not path.is_file():
            raise ValueError('benchmark input artifact missing; rerun the baseline')
        if oracle.file_digest(path)!=case['input_sha256'] or path.stat().st_size!=case['file_bytes']:
            raise ValueError('benchmark input bytes changed')
        for kind in ('reads','writes','write_validations'):
            if len(case[kind])!=5: raise ValueError('incomplete benchmark trials')
        for result in [case['warmup'],*case['reads'],*case['write_validations']]: validate_scan(result,workload)
        for result in [case['generation'],*case['writes']]:
            if result['rows']!=workload['rows'] or result['elapsed_seconds']<=0: raise ValueError('invalid write trial')
        for key,trials,field in [('scan_seconds',case['reads'],'scan_seconds'),('open_seconds',case['reads'],'open_seconds'),
                                 ('metadata_seconds',case['reads'],'metadata_seconds'),('write_seconds',case['writes'],'elapsed_seconds')]:
            if case[key]!=summary([trial[field] for trial in trials]): raise ValueError('benchmark summary differs from raw trials')
        seconds=case['scan_seconds']['median']
        decoded=workload['rows']*(workload['dimension']*4+workload['fields']*8)
        if case['nominal_decoded_bytes']!=decoded or case['records_per_second']!=workload['rows']/seconds:
            raise ValueError('benchmark work/throughput summary changed')
        if case['file_mib_per_second']!=case['file_bytes']/(1024*1024)/seconds or case['decoded_mib_per_second']!=decoded/(1024*1024)/seconds:
            raise ValueError('benchmark byte throughput changed')
    return dict(status='verified',scenarios=4,measured_scans=20,measured_writes=20,
                write_validation_scans=20,large_profile='unrun',revision=report['revision'])
