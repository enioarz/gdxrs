"""Task 038 Linux worker/stack smoke diagnostics, not M6 scale acceptance."""
import json,platform,shutil,subprocess,sys,uuid
from pathlib import Path
import m5_scale as m5
import oracle,scale,writer_scale

ROOT=Path(__file__).resolve().parents[1]
SOURCES=sorted(set(m5.SOURCES+['tools/parallel_check.py','examples/parallel_worker.rs','tools/tests/test_parallel_check.py']))

def validate(trial,workers,query,expected):
    r=trial['result']
    if trial['exit_code'] or r['query']!=query or {k:r[k]for k in ['count','digest']}!=expected or r['returned_rows']!=r['count']:
        raise ValueError('parallel full ordered result mismatch')
    admitted=0 if query=='zero'else workers
    if (not r['os_cap_verified'] or r['root_limit']!=m5.BUDGET or not 0<r['root_peak']<=m5.BUDGET
            or trial['os_cap']['soft_bytes']!=m5.CAP or trial['os_cap']['hard_bytes']!=m5.CAP
            or not 0<trial['peak_rss_bytes']<=m5.CAP or not 0<r['vm_peak_bytes']<=m5.CAP
            or trial['sampled_peak_fds']>5):
        raise ValueError('parallel process/root/descriptor containment missing')
    if (r['requested']!=workers or r['admitted']!=admitted or r['joined']!=admitted
            or r['maximum_buffered_batches']!=admitted or r['peak_ready_batches']>admitted
            or r['stack_reserved']!=admitted*(2*1024**2+64*1024)
            or r['control_reserved']<admitted*256*1024):
        raise ValueError('parallel worker/stack/queue admission mismatch')
    if r['complete']!=(query!='cancel') or r['cancelled']!=(query=='cancel'):
        raise ValueError('parallel cancellation/exhaustion confused')
    if query=='all'and(r['decoded_rows']!=r['count']or r['produced_rows']!=r['count']or r['tasks_dispatched']!=r['tasks_completed']):
        raise ValueError('parallel full scan did not finish exactly')
    if query=='zero'and(r['decoded_rows']or r['query_bytes_read']or r['produced_rows']or r['tasks_dispatched']):
        raise ValueError('zero limit did record work')

def validate_matrix(cases):
    if len(cases)!=2 or {c['codec']for c in cases}!={'raw','zlib'}:
        raise ValueError('both raw and zlib families required')
    want={(workers,query)for workers in [2,4]for query in ['all','head','zero','cancel']}
    for c in cases:
        if len(c['trials'])!=8 or {(t['result']['requested'],t['result']['query'])for t in c['trials']}!=want:
            raise ValueError('complete worker/query matrix required')

def check(run,profile):
    if sys.platform!='linux'or profile not in ['smoke','small']:raise ValueError('parallel-check requires Linux smoke/small')
    revision=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
    hashes={p:oracle.file_digest(ROOT/p)for p in SOURCES}
    if profile=='small'and subprocess.check_output(['git','status','--porcelain','--',*SOURCES],text=True).strip():
        raise ValueError('commit measured sources before small diagnostics')
    area=ROOT/'.work/parallel-check'/uuid.uuid4().hex;area.mkdir(parents=True)
    native,_,reference=writer_scale.build(run,area)
    run(['cargo','build','--locked','--release','--features','index','--example','parallel_worker','--example','m5_worker'])
    bins={}
    for name in ['parallel_worker','m5_worker']:
        p=area/name;shutil.copyfile(ROOT/'target/release/examples'/name,p);p.chmod(0o700);bins[name]=p
    cases=[];n=32768 if profile=='smoke'else 1_000_000;stride=256 if profile=='smoke'else 4096
    for codec in ['raw','zlib']:
        source,index=area/(codec+'.gdx'),area/(codec+'.index')
        recipe=json.loads(run([native,'write',source,'records',n,codec],capture=True).stdout)
        identity=writer_scale.hash_record(source)
        native_validation=scale.measure([native,'native',source,'records',n],area,codec+'-native')
        if native_validation['result']['all']!=recipe['all']:raise ValueError('native full source mismatch')
        built=scale.measure([bins['m5_worker'],'build',source,index,'records',n,stride,'all',0],area,codec+'-build');m5.validate_worker(built)
        phase=m5.phases(built)[0]
        if not phase['complete']or not phase['published']or phase['rows_decoded']!=n:raise ValueError('incomplete explicit build')
        with index.open('rb')as f:header=f.read(1024)
        if header[24:56].hex()!=identity['sha256']:raise ValueError('independent source/header digest mismatch')
        case=dict(codec=codec,count=n,source=identity,recipe=recipe,native_validation=native_validation,build=built,trials=[])
        for workers in [2,4]:
            for query in ['all','head','zero','cancel']:
                t=scale.measure([bins['parallel_worker'],source,index,n,stride,workers,query],area,f'{codec}-{workers}-{query}')
                if query=='all':expected=recipe['all']
                elif query=='head':expected=scale.expected(13)['all']
                elif query=='zero':expected=dict(count=0,digest=f'{scale.INITIAL:016x}')
                else:
                    if not 0<t['result']['count']<=1024:raise ValueError('unbounded cancelled prefix')
                    expected=scale.expected(t['result']['count'])['all']
                validate(t,workers,query,expected);case['trials'].append(t)
        if writer_scale.hash_record(source)!=identity:raise ValueError('parallel mutated source')
        case['index']=writer_scale.hash_record(index);cases.append(case)
        (area/'progress.json').write_text(json.dumps(dict(cases=cases,qualified=False),indent=2)+'\n')
        index.unlink();source.unlink()
    if hashes!={p:oracle.file_digest(ROOT/p)for p in SOURCES}:raise ValueError('sources changed during parallel diagnostics')
    validate_matrix(cases)
    return dict(source_revision=revision,source_hashes=hashes,profile=profile,cases=cases,oracle=reference,
        binary_hashes={k:oracle.file_digest(p)for k,p in bins.items()},compiler=run(['rustc','--version','--verbose'],capture=True).stdout,
        hardware=dict(platform=platform.platform(),cpu=next(l.split(':',1)[1].strip()for l in Path('/proc/cpuinfo').read_text().splitlines()if l.startswith('model name'))),
        storage=run(['df','-T',ROOT],capture=True).stdout,root=str(area.relative_to(ROOT)),
        scope='Functional two/four-worker diagnostics under one 256 MiB root and enforced 512 MiB Linux process cap; not independently oversized or performance acceptance')
