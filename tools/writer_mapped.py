"""Task 026: bounded recipes with complete raw/mapped expectations, <=2048 UELs."""
import hashlib
import json
import sys
import subprocess
import re
from pathlib import Path
import fixtures
from oracle_limits import isolation_policy
ROOT=Path(__file__).resolve().parents[1]
Z='f:0000000000000000';O='f:3ff0000000000000';P='f:3ff4000000000000'
def labels(large):return [f'u{n}'.encode() for n in range(1,2049)] if large else [b'z',b'a',b'C']
def mapped(raw,large):return ((raw*17)%2048)*7 if large else {1:9,2:0,3:3}[raw]
def spec(large):
    members=[([n],[Z]) for n in range(1,2049 if large else 4)]
    if large:return [(b'I',1,0,0,b'',1,['0:2a'],members)]
    return [(b'I',1,0,0,b'',1,['0:2a'],members),(b'J',1,4,1,b'Aliased with I',1,['0:2a'],members),
      (b'P',2,1,0,b'',3,['2:4a','0:2a'],[([1,2],[P]),([2,3],['s:eps']),([3,1],['s:undef'])]),
      (b'V',20,2,5,b'',1,['0:2a']*20,[([1]*20,['a:7','f:bff0000000000000','s:undef','s:pinf','f:4004000000000000'])]),
      (b'E',0,3,53,b'',1,[],[([],['s:na',Z,Z,Z,O])]),(b'Empty',0,1,0,b'',1,[],[([],[Z])])]
def raw_expected(compression,producer,audit,large):
    uels=labels(large);symbols=spec(large)
    lines=['GDXRS-ORACLE\t1',f'F\t7\t{compression}\t{len(symbols)}\t{len(uels)}\t{producer}\t{audit}']
    lines += [f'U\t{n}\t'+b.hex() for n,b in enumerate(uels,1)]+['T\t0\t0\t']
    if not large:lines+=['A\t1\t7\t4163726f\t7479706564']
    lines += [f'S\t0\t2a\t1\t0\t{len(uels)}\t0\t556e697665727365']+[f'R\t0\t{n}\t{Z}' for n in range(1,len(uels)+1)]+[f'E\t0\t{len(uels)}']
    for n,(name,dim,kind,user,text,mode,domains,rows) in enumerate(symbols,1):
        lines += [f'S\t{n}\t{name.hex()}\t{dim}\t{kind}\t{0 if kind==4 else len(rows)}\t{user}\t{text.hex()}',f'D\t{n}\t{mode}'+''.join('\t'+d for d in domains)]
        lines += [f'R\t{n}\t'+','.join(map(str,k))+'\t'+','.join(v) for k,v in rows]+[f'E\t{n}\t{len(rows)}']
    return '\n'.join(lines)+'\n'
def mapped_expected(symbol,large,skip=False):
    rows=spec(large)[symbol-1][-1];accepted=[];skipped=0
    for raw,values in rows:
        if skip and 2 in raw:skipped+=1;continue
        accepted.append(([mapped(n,large) for n in raw],raw,values))
    accepted.sort(key=lambda r:r[0])
    lines=['GDXRS-MAPPED\t1']+['R\t'+','.join(map(str,m))+'\t'+','.join(v)+'\t'+','.join(map(str,r)) for m,r,v in accepted]+[f'E\t{len(accepted)}\t{skipped}']
    return '\n'.join(lines)+'\n'
def recipe(large):
    lines=[f'M\t{mapped(n,large)}\t{b.hex()}' for n,b in enumerate(labels(large),1)]
    if not large:lines+=['M\t9\t7a','A\t4163726f\t7479706564\t7']
    for name,dim,kind,user,text,mode,domains,rows in spec(large):
        if kind==4:lines+=['ALIAS\t49\t4a'];continue
        lines+=[f'S\t{name.hex()}\t{dim}\t{kind}\t{user}\t{text.hex()}\tmap']
        if mode==3:lines+=['D\t4a\t2a']
        if name!=b'Empty':lines+=['K\t'+','.join(str(mapped(n,large)) for n in raw)+'\t'+','.join(values) for raw,values in reversed(rows)]
        lines+=['END']
    return '\n'.join(lines)+'\n'
def check(run,area,executable):
    cases=[]
    probe=area/'map-probe.tsv';probe.write_text('M\t0\t61\nM\t9\t7a\nM\t9\t62\nM\t0\t61\n!M\t1\t61\nS\t50\t1\t1\t0\t\tmap\nK\t9\t'+O+'\nEND\n')
    probe_file=area/'map-probe.gdx';probe_result=run([executable,'recipe',probe_file,probe,'0'],capture=True).stdout
    probe_dump=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',probe_file],capture=True,timeout=5).stdout
    if len(probe_result.splitlines())!=1 or f'R\t1\t3\t{O}\n' not in probe_dump:raise ValueError('native map zero/collision/negative contract changed')
    negative_recipe=area/'map-negative.tsv';negative_recipe.write_text('M\t0\t61\n!M\t-1\t6e6567\n')
    negative=subprocess.run([str(sys.executable),str(ROOT/'tools/oracle_limits.py'),str(executable),'recipe',str(area/'map-negative.gdx'),str(negative_recipe),'0'],capture_output=True,text=True,timeout=5)
    if negative.returncode not in [0,-6,-11] or negative.returncode==0 and 'REJECT' not in negative.stdout:raise ValueError('native negative-map probe had an unexpected outcome; investigate source/contract')
    negative_probe=dict(returncode=negative.returncode,stderr_tail=negative.stderr[-256:],isolation=isolation_policy(),rust_rejects_before_registration=True)
    for large in [False,True]:
        recipe_path=area/f'mapped-{large}.tsv';recipe_path.write_text(recipe(large))
        for compression in [0,1]:
            native_path=area/f'mapped-native-{large}-{compression}.gdx';run([executable,'recipe',native_path,recipe_path,str(compression)],capture=True)
            native_raw=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',native_path],capture=True,timeout=10).stdout
            if fixtures.normalize_dump(native_raw)!=raw_expected(compression,b'gdxrs-fixtures'.hex(),'<creation-system>',large):raise ValueError('native mapped author raw stream mismatch')
            features=[[]] if large else [[],['--all-features']]+([['--no-default-features']] if not compression else [])
            for feature in features:
                rust_raw=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',native_path],capture=True).stdout
                if rust_raw!=raw_expected(compression,b'gdxrs-fixtures'.hex(),'<creation-system>',large):raise ValueError('native mapped write / Rust raw read mismatch')
                path=area/f'mapped-rust-{len(cases)}.gdx'
                created=run(['cargo','run','--locked','--release','--quiet',*feature,'--example','create_mapped','--',path,area,*(['zlib'] if compression else []),*(['large'] if large else [])],capture=True)
                stats=json.loads(created.stdout)
                if not 0<stats['peak_root']<=1048576 or not 0<stats['peak_handles']<=7:raise ValueError('mapped author root/handles exceeded')
                if path.stat().st_size>1048576:raise ValueError('mapped fixture exceeds fixed small-artifact bound')
                nr=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',path],capture=True,timeout=10).stdout
                rr=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',path],capture=True).stdout
                if nr!=raw_expected(compression,b'gdxrs'.hex(),b'gdxrs'.hex(),large) or rr!=raw_expected(compression,b'gdxrs'.hex(),'<creation-system>',large):raise ValueError('Rust mapped write / native/Rust raw read mismatch')
                checks=[]
                for source in [native_path,path]:
                    for skip in ([False] if large else [False,True]):
                        mapping=area/f'caller-{large}-{skip}.tsv';mapping.write_text(''.join(f'{mapped(n,large)}\t{b.hex()}\n' for n,b in enumerate(labels(large),1) if not skip or n!=2))
                        for symbol in ([1] if large else [1,3,4,5,6]):
                            native=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'mapped-dump',source,mapping,str(symbol)],capture=True,timeout=10).stdout
                            rust=run(['cargo','run','--locked','--quiet',*feature,'--example','dump_mapped','--',source,area,str(symbol),*(['large'] if large else []),*(['skip'] if skip else [])],capture=True)
                            expected=mapped_expected(symbol,large,skip)
                            if native!=expected or rust.stdout!=expected:
                                (area/'mapped-native-actual.tsv').write_text(native);(area/'mapped-rust-actual.tsv').write_text(rust.stdout);(area/'mapped-expected.tsv').write_text(expected);raise ValueError('complete mapped order/raw identity/value/skipped stream mismatch')
                            resources=re.fullmatch(r'mapped_peak_root=(\d+) mapped_peak_handles=(\d+)\n',rust.stderr)
                            if resources is None or not 0<int(resources[1])<=1048576 or not 0<int(resources[2])<=7:raise ValueError('mapped read root/handles exceeded')
                            checks.append(dict(peak_root=int(resources[1]),peak_handles=int(resources[2]),symbol=symbol,source='native' if source==native_path else 'rust',skip_missing=skip,sha256=hashlib.sha256(expected.encode()).hexdigest()))
                with path.open('rb') as f:digest=hashlib.file_digest(f,'sha256').hexdigest()
                cases.append(dict(features=feature,compression=compression,large=large,statistics=stats,sha256=digest,mapped_streams=checks,both_directions_complete_match=True))
    return dict(cases=cases,negative_probe=negative_probe,native_probe=probe_result,native_probe_raw_sha256=hashlib.sha256(probe_dump.encode()).hexdigest(),native_isolation=isolation_policy(),scope='immutable sparse bijections, original raw IDs, mapped order, strict/skip missing policy; native ambiguous collisions explicitly rejected in Rust')
