"""Task 027: independent bounded domain/error recipes; <=3 UELs, 10 rows."""
import hashlib
import json
import sys
from pathlib import Path
import fixtures
import oracle
from oracle_limits import isolation_policy
ROOT=Path(__file__).resolve().parents[1]
Z='f:0000000000000000'
REJECTIONS=[(2,[1,1],0,'s:eps'),(3,[2,1],1,'s:na'),(4,[2,1],1,'s:undef'),(5,[2,2],3,'s:minf'),(6,[3,1],1,'s:pinf'),(7,[3,3],3,'f:4004000000000000')]
def errors(native=False,cutoff=100):
    rows=[r for r in REJECTIONS if not native or r[0]!=4]
    if native:rows=rows[:cutoff]
    lines=['GDXRS-DOMAIN-ERRORS\t1' if native else 'GDXRS-RUST-ERRORS\t1']
    if native:lines += [f'A\t{n}\t{int(n==1)}' for n in range(1,8)]
    lines += [f'X\t{index if native else ordinal}\t'+','.join(map(str,keys))+f'\t{mask}\t{value}' for index,(ordinal,keys,mask,value) in enumerate(rows,1)]
    if native:lines += [f'E\t{len(rows)}']
    return '\n'.join(lines)+'\n'
def expected(compression,producer,audit,forward=False):
    specs=[('49',1,0,0,'',3,['1:49'],[([1],[Z])]),
      ('4a',1,4,1,'416c696173656420776974682049',1,['0:2a'],[([1],[Z])]),
      ('50',2,1,0,'',3,['2:4a','1:49'],[([1,1],['f:3ff4000000000000'])]),
      ('52',1,1,0,'',2,['0:556e72656769737465726564'],[([3],['s:eps'])]),
      ('55',1,1,0,'',3,['0:2a'],[([3],['s:na'])])]
    if forward:
        specs=[('50',2,1,0,'',3,['3:4a','2:49'],[([1,1],['f:3ff4000000000000'])]),
          ('49',1,0,0,'',3,['2:49'],[([1],[Z])]),
          ('4a',1,4,2,'416c696173656420776974682049',1,['0:2a'],[([1],[Z])]),*specs[3:]]
    lines=['GDXRS-ORACLE\t1' ,f'F\t7\t{compression}\t5\t3\t{producer}\t{audit}','U\t1\t61','U\t2\t62','U\t3\t63','T\t0\t0\t','S\t0\t2a\t1\t0\t3\t0\t556e697665727365']
    lines += [f'R\t0\t{n}\t{Z}' for n in range(1,4)]+['E\t0\t3']
    for n,(name,dim,kind,user,text,mode,domains,rows) in enumerate(specs,1):
        lines += [f'S\t{n}\t{name}\t{dim}\t{kind}\t{0 if kind==4 else len(rows)}\t{user}\t{text}',f'D\t{n}\t{mode}'+''.join('\t'+d for d in domains)]
        lines += [f'R\t{n}\t'+','.join(map(str,k))+'\t'+','.join(v) for k,v in rows]+[f'E\t{n}\t{len(rows)}']
    return '\n'.join(lines)+'\n'
def build(run,area):
    native=json.loads((oracle.AREA/'current.json').read_text());library=Path(native['executable']).parent.parent/'gdx/libgdx-static.a';directory=area/'domains-driver'
    run([oracle.cmake(),'-S',ROOT/'tools/domains','-B',directory,'-DCMAKE_BUILD_TYPE=Release',f'-DGDX_ROOT={oracle.AREA/"gdx"}',f'-DGDX_LIBRARY={library}'],capture=True)
    run([oracle.cmake(),'--build',directory,'--parallel','2'],capture=True,timeout=180)
    executable=directory/'gdxrs-domains';digest=hashlib.sha256(oracle.source_fingerprint().encode())
    for path in ['tools/domains/CMakeLists.txt','tools/domains/driver.cpp']:digest.update((ROOT/path).read_bytes())
    return executable,dict(source_fingerprint=digest.hexdigest(),executable_sha256=oracle.file_digest(executable),frozen_oracle=oracle.file_digest(oracle.executable()))
def check(run,area,executable):
    helper,identity=build(run,area);cases=[];probes=[]
    for compression in [0,1]:
        for cutoff in [100,2]:
            path=area/f'domains-native-{compression}-{cutoff}.gdx'
            result=run([sys.executable,ROOT/'tools/oracle_limits.py',helper,path,str(compression),str(cutoff)],capture=True,timeout=10).stdout
            if result!=errors(True,cutoff):raise ValueError('native accepted/error stream, coalescing or cutoff expectation changed')
            ndump=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',path],capture=True,timeout=10).stdout
            if fixtures.normalize_dump(ndump)!=expected(compression,b'gdxrs-domains'.hex(),'<creation-system>'):raise ValueError('native complete domain metadata/accepted records changed')
            features=[[],['--all-features']]+([['--no-default-features']] if not compression else [])
            for feature in features:
                rdump=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',path],capture=True).stdout
                if rdump!=expected(compression,b'gdxrs-domains'.hex(),'<creation-system>'):raise ValueError('native write/Rust read mismatch')
            probes.append(dict(compression=compression,cutoff=cutoff,error_sha256=hashlib.sha256(result.encode()).hexdigest(),file_sha256=oracle.file_digest(path),rust_reads=len(features)))
        for forward,feature in [(forward,feature) for forward in [False,True] for feature in features]:
            path=area/f'domains-rust-{len(cases)}.gdx'
            created=run(['cargo','run','--locked','--quiet',*feature,'--example','create_domains','--',path,area,*(['zlib'] if compression else []),*(['forward'] if forward else [])],capture=True)
            if created.stdout!=errors():raise ValueError('Rust lossless error stream, ordinals or values mismatch')
            stats=json.loads(created.stderr)
            if (stats['input'],stats['accepted'],stats['reported'])!=(10,4,6) or not 0<stats['peak_root']<=1048576 or not 0<stats['peak_handles']<=7:raise ValueError('domain counters/root/handles exceeded')
            if path.stat().st_size>1048576:raise ValueError('domain recipe exceeds fixed small-artifact bound')
            native=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',path],capture=True,timeout=10).stdout
            rust=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',path],capture=True).stdout
            if native!=expected(compression,b'gdxrs'.hex(),b'gdxrs'.hex(),forward) or rust!=expected(compression,b'gdxrs'.hex(),'<creation-system>',forward):raise ValueError('Rust write/native read domain metadata or accepted stream mismatch')
            cases.append(dict(compression=compression,forward_registration=forward,features=feature,statistics=stats,file_sha256=oracle.file_digest(path),errors_sha256=hashlib.sha256(created.stdout.encode()).hexdigest(),both_directions_complete_match=True))
    return dict(cases=cases,native_probes=probes,driver=identity,native_isolation=isolation_policy(),scope='strict alias/self/universe and relaxed domains; duplicates; native signed/absolute errors and cutoff/coalescing; Rust preserves every rejected row')
