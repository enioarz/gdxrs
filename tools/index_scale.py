"""Task 033 paged directory containment; not M5 indexed-query acceptance."""
import hashlib,json,platform,shutil,subprocess,sys,uuid
from pathlib import Path
import scale,writer_scale,oracle
ROOT=Path(__file__).resolve().parents[1]
CAP=64*1024**2
BUDGET=8*1024**2
SOURCES=sorted(['Cargo.toml','Cargo.lock','rust-toolchain.toml','examples/index_worker.rs','tools/index_scale.py','tools/index_dependencies.py','tools/dev.py','tools/scale.py','tools/writer_scale.py','tools/scale/driver.cpp','tools/scale/CMakeLists.txt']+[str(p.relative_to(ROOT)) for p in (ROOT/'src').rglob('*.rs')])
def validate(trial,n):
 r=trial['result']
 if r['count']!=n or r['intervals']!=n or r['root_limit']!=BUDGET or r['root_peak']>BUDGET or not r['os_cap_verified']:raise ValueError('index containment/counts failed')
 if trial['os_cap']['hard_bytes']!=CAP or trial['os_cap']['soft_bytes']!=CAP or trial['peak_rss_bytes']>CAP:raise ValueError('index OS cap missing')
 if r['index_bytes']>2*1024**3:raise ValueError('index quota exceeded')
def measure(argv,area,name):
 old=scale.CAP
 try:
  scale.CAP=CAP
  return scale.measure(argv,area,name,timeout=7200)
 finally:scale.CAP=old

def qualify(cases):
 if len(cases)<2 or cases[-1]['count']<=cases[0]['count']:raise ValueError('growing directory cases required')
 last=cases[-1]
 if min(last['build']['result']['directory_bytes'],last['opened']['result']['directory_bytes'])<=CAP:raise ValueError('directory itself must exceed process RAM cap')
 if last['index']['bytes']<=4*CAP or last['source']['bytes']<=CAP:raise ValueError('independently oversized source/index missing')
 for phase in ['build','opened']:
  if cases[-1][phase]['peak_rss_bytes']>cases[0][phase]['peak_rss_bytes']+8*1024**2:raise ValueError('index RSS growth exceeds fixed 8 MiB allowance')

def check(run,profile):
 if sys.platform!='linux' or profile not in ['smoke','scale']:raise ValueError('index containment requires Linux smoke/scale')
 revision=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
 hashes={p:oracle.file_digest(ROOT/p) for p in SOURCES}
 if profile=='scale' and subprocess.check_output(['git','status','--porcelain','--',*SOURCES],text=True).strip():raise ValueError('commit measured sources before scale')
 area=ROOT/'.work/index-acceptance'/uuid.uuid4().hex;area.mkdir(parents=True)
 native,_,reference=writer_scale.build(run,area)
 run(['cargo','build','--locked','--release','--features','index','--example','index_worker'])
 binary=area/'index_worker';shutil.copyfile(ROOT/'target/release/examples/index_worker',binary);binary.chmod(0o700)
 counts=[1024,4096] if profile=='smoke' else [300_000,1_200_000]
 cases=[]
 for n in counts:
  source=area/f'source-{n}.gdx';index=area/f'index-{n}'
  recipe=json.loads(run([native,'generate',source,'records',n],capture=True,timeout=1800).stdout)
  native_verify=measure([native,'native',source,'records',n],area,f'native-{n}')
  if native_verify['result']['all']!=recipe['expected']['all']:raise ValueError('native complete source validation failed')
  source_identity=writer_scale.hash_record(source)
  build=measure([binary,'build',source,index,n,CAP],area,f'build-{n}');validate(build,n)
  opened=measure([binary,'open',source,index,n,CAP],area,f'open-{n}');validate(opened,n)
  with index.open('rb') as f:header=f.read(1024)
  if header[24:56].hex()!=source_identity['sha256']:raise ValueError('independent SHA-256 source identity mismatch')
  if writer_scale.hash_record(source)!=source_identity:raise ValueError('index modified source')
  identity=writer_scale.hash_record(index)
  if build['result']['index_bytes']!=identity['bytes'] or opened['result']['index_bytes']!=identity['bytes']:raise ValueError('index physical extent mismatch')
  cases.append(dict(count=n,source=source_identity,index=identity,build=build,opened=opened,native_validation=native_verify))
  (area/'progress.json').write_text(json.dumps(dict(revision=revision,source_hashes=hashes,cases=cases,index_directory_qualified=False),indent=2)+'\n')
  index.unlink();source.unlink()
 if profile=='scale':qualify(cases)
 if hashes!={p:oracle.file_digest(ROOT/p) for p in SOURCES}:raise ValueError('index sources changed during qualification')
 return dict(source_revision=revision,source_hashes=hashes,profile=profile,index_directory_qualified=profile=='scale',milestone_m5_qualified=False,root=str(area.relative_to(ROOT)),process_cap=CAP,library_budget=BUDGET,cases=cases,binary_sha256=oracle.file_digest(binary),native_binary_sha256=oracle.file_digest(native),oracle=reference,compiler=run(['rustc','--version','--verbose'],capture=True).stdout,hardware=dict(platform=platform.platform(),cpu=next(l.split(':',1)[1].strip() for l in Path('/proc/cpuinfo').read_text().splitlines() if l.startswith('model name'))),storage=run(['df','-T',ROOT],capture=True).stdout,retention='Generated artifacts retired after complete validation and SHA-256; compact reports retained',scope='Single-symbol paged directory containment under 8 MiB root / enforced 64 MiB process; task 035 retains 256/512 MiB >4x milestone inputs and cold/warm query gates')
