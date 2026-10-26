"""Task 023: Linux OS-capped sorted writer acceptance and native phase timings."""
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import uuid
import oracle
import scale

ROOT=Path(__file__).resolve().parents[1]
CAP=512*1024*1024
BUDGET=256*1024*1024
NATIVE_METADATA_CAP=16*1024*1024*1024
SOURCES=['tools/writer_scale.py','tools/scale.py','tools/scale/driver.cpp','tools/scale/CMakeLists.txt','examples/scale_writer.rs','examples/scale_sink.rs','Cargo.toml','Cargo.lock','rust-toolchain.toml']+sorted(str(p.relative_to(ROOT)) for p in (ROOT/'src').rglob('*.rs'))

def measure(argv,area,name,*,reference_metadata=False):
    # Rust workers self-check 512 MiB. The native implementation eagerly owns
    # metadata; its separate reference process has an explicitly larger cap.
    old=scale.CAP
    try:
        if reference_metadata:scale.CAP=NATIVE_METADATA_CAP
        return scale.measure(argv,area,name,timeout=3600)
    finally:scale.CAP=old

def validate_worker(trial,count,labels,metadata=False,pipeline=False):
    r=trial['result']
    if r['count']!=(0 if metadata else count) or r['labels']!=labels:raise ValueError('writer counts mismatch')
    if not r['os_cap_verified'] or r['root_limit']!=BUDGET or r['root_peak']>BUDGET:raise ValueError('writer root/OS containment mismatch')
    if trial['os_cap']['hard_bytes']!=CAP or trial['peak_rss_bytes']>CAP:raise ValueError('writer OS cap missing')
    if r['shared_root']!=pipeline:raise ValueError('pipeline did not use one root')
    if r['scratch_peak']>16*1024*1024*1024 or r['scratch_peak_handles']>12:raise ValueError('scratch quota/handle excess')

def qualify_large(case,kind):
    if case['logical_bytes']<=4*CAP or case['bytes']<=4*CAP:raise ValueError('largest physical AND logical writer datasets must exceed four times cap')
    if kind=='metadata' and case['labels']*64<=4*CAP:raise ValueError('metadata is not independently oversized')

def growth(cases):
    if len(cases)<2 or cases[-1]['count']<=cases[0]['count']:raise ValueError('growing sizes required')
    if cases[-1]['worker']['peak_rss_bytes']>cases[0]['worker']['peak_rss_bytes']+64*1024*1024:raise ValueError('writer RSS growth exceeds fixed 64 MiB allowance')

def build(run,area):
    oracle.build(run)
    ref=json.loads((ROOT/'.work/oracle/current.json').read_text())
    library=Path(ref['executable']).parents[1]/'gdx/libgdx-static.a'
    native=area/'native-build'
    run([oracle.cmake(),'-S',ROOT/'tools/scale','-B',native,'-DCMAKE_BUILD_TYPE=Release',f'-DGDX_ROOT={ROOT/".work/oracle/gdx"}',f'-DGDX_LIBRARY={library}'])
    run([oracle.cmake(),'--build',native,'--parallel','2'])
    run(['cargo','build','--locked','--release','--example','scale_writer','--example','scale_sink'])
    bins={}
    for name in ['scale_writer','scale_sink']:
        path=area/name;shutil.copyfile(ROOT/'target/release/examples'/name,path);path.chmod(0o700);bins[name]=path
    return native/'gdxrs-scale',bins,ref

def hash_record(path):
    return dict(bytes=path.stat().st_size,sha256=oracle.file_digest(path))

def check(run,profile):
    if sys.platform!='linux':raise ValueError('writer acceptance OS containment is Linux-only')
    if profile not in ['smoke','scale']:raise ValueError('invalid writer scale profile')
    revision=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
    hashes={p:oracle.file_digest(ROOT/p) for p in SOURCES}
    if profile=='scale' and subprocess.check_output(['git','status','--porcelain','--',*SOURCES],text=True).strip():raise ValueError('commit measured sources before scale qualification')
    area=ROOT/'.work/writer-acceptance'/uuid.uuid4().hex;area.mkdir(parents=True)
    native,bins,ref=build(run,area)
    # Calibration is independently calculated in Python, before large recipes.
    calibration=[]
    for kind,n in [('records',8192),('metadata',1024)]:
        path=area/f'calibration-{kind}.gdx'
        recipe=json.loads(run([native,'generate',path,kind,n],capture=True).stdout)
        if kind=='records' and recipe['expected']!=scale.expected(n):raise ValueError('independent record recipe calibration')
        if recipe['label_digest']!=scale.expected_labels(recipe['labels']):raise ValueError('independent label recipe calibration')
        path.unlink();calibration.append(recipe)
    # Repeated same-work phase comparisons. OS page cache is reused; output is
    # freshly created and Rust finish includes sync_all, native gdxClose does not
    # promise equivalent durability. Report that policy rather than hide it.
    benchmarks=[]
    for kind,n,compression in [('records',1_000_000,'raw'),('records',1_000_000,'zlib'),('metadata',250_000,'raw')]:
        if profile=='smoke':n=min(n,16384)
        trials={p:[] for p in ['rust','native']}
        for i in range(3):
            for producer in ['native','rust'] if i%2 else ['rust','native']:
                path=area/f'bench-{kind}-{compression}-{producer}-{i}.gdx';scratch=area/f'scratch-{uuid.uuid4().hex}';scratch.mkdir()
                argv=[bins['scale_writer'],kind,path,scratch,n,compression] if producer=='rust' else [native,'write',path,kind,n,compression]
                trial=measure(argv,area,f'bench-{kind}-{compression}-{producer}-{i}')
                if producer=='rust':validate_worker(trial,n,n if kind=='metadata' else max(1024,(n+1023)//1024),metadata=kind=='metadata')
                trials[producer].append(trial);path.unlink();scratch.rmdir()
        for i in range(3):
            a,b=trials['rust'][i]['result'],trials['native'][i]['result']
            if a['label_digest']!=b['label_digest'] or dict(count=a['count'],digest=a['digest'])!=b['all']:raise ValueError('benchmark complete generated digest mismatch')
        phases={p:{phase:scale.summary([t['result'][phase] for t in rows]) for phase in ['registration_seconds','encoding_seconds','finalization_seconds']} for p,rows in trials.items()}
        benchmarks.append(dict(kind=kind,count=n,compression=compression,trials=trials,phases=phases))
    record_counts=[4096,16384] if profile=='smoke' else [13_000_000,52_000_000]
    metadata_counts=[4096,16384] if profile=='smoke' else [9_000_000,34_000_000]
    cases=[];record_growth=[];metadata_growth=[]
    for n in record_counts:
        source=area/f'input-{n}.gdx'
        recipe=json.loads(run([native,'generate',source,'records',n],capture=True,timeout=1800).stdout)
        source_identity=hash_record(source)
        for mode,compression in [('records','raw'),('records','zlib'),('pipeline','raw')]:
            output=area/f'{mode}-{compression}-{n}.gdx';scratch=area/f'scratch-{uuid.uuid4().hex}';scratch.mkdir()
            argv=[bins['scale_writer'],mode,output,scratch,n,compression]+([source] if mode=='pipeline' else [])
            trial=measure(argv,area,f'{mode}-{compression}-{n}')
            validate_worker(trial,n,recipe['labels'],pipeline=mode=='pipeline')
            if dict(count=trial['result']['count'],digest=trial['result']['digest'])!=recipe['expected']['all']:raise ValueError('writer ordered record digest mismatch')
            verify=measure([native,'native',output,'records',n],area,f'verify-{mode}-{compression}-{n}')
            if verify['result']['all']!=recipe['expected']['all'] or verify['result']['label_digest']!=recipe['label_digest']:raise ValueError('native complete records/metadata IDs mismatch')
            rust=measure([bins['scale_sink'],'all',output,n,scratch],area,f'readback-{mode}-{compression}-{n}')
            scale.validate_scan(rust,recipe,'all')
            case=dict(kind=mode,compression=compression,count=n,labels=recipe['labels'],logical_bytes=n*120,worker=trial,native_validation=verify,rust_validation=rust,input=source_identity,**hash_record(output))
            cases.append(case)
            if mode=='records' and compression=='raw':record_growth.append(case)
            output.unlink();scratch.rmdir()
        source.unlink()
    for n in metadata_counts:
        output=area/f'metadata-{n}.gdx';scratch=area/f'scratch-{uuid.uuid4().hex}';scratch.mkdir()
        trial=measure([bins['scale_writer'],'metadata',output,scratch,n,'raw'],area,f'metadata-{n}')
        validate_worker(trial,n,n,metadata=True)
        native_verify=measure([native,'native-labels',output,'metadata',n],area,f'verify-metadata-{n}',reference_metadata=True)
        if native_verify['result']!=dict(count=n,digest=trial['result']['label_digest']):raise ValueError('native complete metadata ID/digest mismatch')
        rust=measure([bins['scale_sink'],'metadata',output,n,scratch],area,f'readback-metadata-{n}')
        if rust['result']['count']!=n or rust['result']['digest']!=native_verify['result']['digest']:raise ValueError('Rust metadata streaming digest mismatch')
        case=dict(kind='metadata',compression='raw',count=n,labels=n,logical_bytes=n*64,worker=trial,native_validation=native_verify,rust_validation=rust,**hash_record(output))
        cases.append(case);metadata_growth.append(case);output.unlink();scratch.rmdir()
    if profile=='scale':
        qualify_large(record_growth[-1],'records');qualify_large(metadata_growth[-1],'metadata')
        pipeline=next(c for c in cases if c['kind']=='pipeline' and c['count']==record_counts[-1]);qualify_large(pipeline,'records')
        growth(record_growth);growth(metadata_growth)
    if hashes!={p:oracle.file_digest(ROOT/p) for p in SOURCES}:raise ValueError('sources changed during qualification')
    return dict(binary_sha256={name:oracle.file_digest(path) for name,path in bins.items()},native_binary_sha256=oracle.file_digest(native),native_compiler=run(['c++','--version'],capture=True).stdout,storage=dict(filesystem=run(['df','-T',ROOT],capture=True).stdout,underlying_device='unavailable behind overlay filesystem'),m3_scale_qualified=profile=='scale',profile=profile,source_revision=revision,source_hashes=hashes,root=str(area.relative_to(ROOT)),calibration=calibration,benchmarks=benchmarks,cases=cases,native_oracle=ref,compiler=run(['rustc','--version','--verbose'],capture=True).stdout,hardware=dict(platform=platform.platform(),cpu=next(l.split(':',1)[1].strip() for l in Path('/proc/cpuinfo').read_text().splitlines() if l.startswith('model name'))),cache_policy='OS cache reused; freshly created outputs; no cold-cache claim',native_metadata_reference_cap=NATIVE_METADATA_CAP,writer_process_cap=CAP,library_budget=BUDGET,retention='Large generated inputs/outputs removed only after full validation and SHA-256; compact reports retained',durability='Rust finalization includes sync_all; native gdxClose durability is not claimed equivalent')
