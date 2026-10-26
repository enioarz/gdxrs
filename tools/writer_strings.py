"""Task-025 independent string recipes; generated dumps bounded below 4 MiB."""
import hashlib
import json
import sys
from pathlib import Path
import fixtures
from oracle_limits import isolation_policy
ROOT=Path(__file__).resolve().parents[1]
ZERO='f:0000000000000000'
ONE='f:3ff0000000000000'
def labels(large, native_author=False):
    return [(f'L{n:08}'+'x'*54).encode() for n in reversed(range(8192))] if large else ([b'z',b'pre',b'a',b'\x80',b'new'] if native_author else [b'z',b'pre',b'a',b'A',b'\x80',b'new'])
def specification(large, native_author=False):
    if large:return [(b'I',1,0,0,b'',1,['0:2a'],[([n],[ZERO]) for n in range(1,8193)])]
    high=5 if native_author else 6
    member=[([n],[ZERO]) for n in ([1,3,4] if native_author else [1,3,4,5])]
    return [(b'I',1,0,0,b'',1,['0:2a'],member),
      (b'J',1,4,1,b'Aliased with I',1,['0:2a'],member),
      (b'P',2,1,0,b'',3,['2:4a','0:2a'],[([3,high],['s:na']),([high-1,3],['s:eps'])]),
      (b'V',20,2,5,b'',1,['0:2a']*20,[([high]*20,['a:7',ZERO if native_author else 'f:8000000000000000','s:undef','s:pinf','f:4004000000000000'])]),
      (b'E',0,3,53,b'',1,[],[([],['s:na',ZERO,ZERO,ZERO,ONE])]),
      (b'Empty',0,1,0,b'',1,[],[([],[ZERO])])]
def expected(compression,producer,audit,large,native_author=False):
    uels=labels(large,native_author); symbols=specification(large,native_author)
    lines=['GDXRS-ORACLE\t1',f'F\t7\t{compression}\t{len(symbols)}\t{len(uels)}\t{producer}\t{audit}']
    lines += [f'U\t{n}\t'+b.hex() for n,b in enumerate(uels,1)]+['T\t0\t0\t']
    if not large:lines+=['A\t1\t7\t4163726f\t7479706564']
    lines += [f'S\t0\t2a\t1\t0\t{len(uels)}\t0\t556e697665727365']
    lines += [f'R\t0\t{n}\t{ZERO}' for n in range(1,len(uels)+1)]+[f'E\t0\t{len(uels)}']
    for n,(name,dim,kind,user,description,mode,domains,rows) in enumerate(symbols,1):
        lines += [f'S\t{n}\t{name.hex()}\t{dim}\t{kind}\t{0 if kind==4 else len(rows)}\t{user}\t{description.hex()}', f'D\t{n}\t{mode}'+''.join('\t'+d for d in domains)]
        lines += [f'R\t{n}\t'+','.join(map(str,k))+'\t'+','.join(v) for k,v in rows]+[f'E\t{n}\t{len(rows)}']
    return '\n'.join(lines)+'\n'
def recipe(large):
    if large:return 'S\t49\t1\t0\t0\t\tstr\n'+''.join('K\t'+b.hex()+'\t'+ZERO+'\n' for b in labels(True))+'END\n'
    lines=['U\t7a','U\t707265','A\t4163726f\t7479706564\t7','S\t49\t1\t0\t0\t\tstr']
    lines+=['K\t'+b.hex()+'\t'+ZERO for b in [b'a',b'A',b'\x80',b'z']]
    lines+=['END','ALIAS\t49\t4a','S\t50\t2\t1\t0\t\tstr','D\t4a\t2a','K\t80,61\ts:eps','K\t61,6e6577\ts:na','END']
    lines+=['S\t56\t20\t2\t5\t\tstr','K\t'+','.join(['6e6577']*20)+'\ta:7,f:8000000000000000,s:undef,s:pinf,f:4004000000000000','END']
    lines+=['S\t45\t0\t3\t53\t\tstr','K\t\ts:na,'+','.join([ZERO]*3+[ONE]),'END','S\t456d707479\t0\t1\t0\t\tstr','END']
    return '\n'.join(lines)+'\n'
def check(run,area,executable):
    cases=[]
    for large in [False,True]:
        recipe_path=area/f'string-{large}.tsv';recipe_path.write_text(recipe(large))
        for compression in [0,1]:
            native_path=area/f'string-native-{large}-{compression}.gdx'
            run([executable,'recipe',native_path,recipe_path,str(compression)],capture=True)
            native=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',native_path],capture=True,timeout=10).stdout
            if fixtures.normalize_dump(native)!=expected(compression,b'gdxrs-fixtures'.hex(),'<creation-system>',large,True):
                (area/'native-string-actual.tsv').write_text(fixtures.normalize_dump(native));(area/'native-string-expected.tsv').write_text(expected(compression,b'gdxrs-fixtures'.hex(),'<creation-system>',large,True));raise ValueError('native string recipe differs from independent expected stream')
            features=[[]] if large else [[],['--all-features']]+([['--no-default-features']] if not compression else [])
            for feature in features:
                rust=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',native_path],capture=True).stdout
                if rust!=expected(compression,b'gdxrs-fixtures'.hex(),'<creation-system>',large,True):raise ValueError('native string write / Rust read mismatch')
                path=area/f'string-rust-{len(cases)}.gdx'
                created=run(['cargo','run','--locked','--release','--quiet',*feature,'--example','create_strings','--',path,area,*(['zlib'] if compression else []),*(['large'] if large else [])],capture=True)
                stats=json.loads(created.stdout)
                if stats['uels']!=len(labels(large)) or stats['records']!=(8192 if large else 8) or not 0<stats['peak_root']<=1048576 or not 0<stats['peak_handles']<=7 or stats['merge_passes']<2:raise ValueError('string resource/count/merge gate failed')
                native=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',path],capture=True,timeout=10).stdout
                rust=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',path],capture=True).stdout
                if native!=expected(compression,b'gdxrs'.hex(),b'gdxrs'.hex(),large) or rust!=expected(compression,b'gdxrs'.hex(),'<creation-system>',large):
                    (area/'string-actual.tsv').write_text(native);(area/'string-expected.tsv').write_text(expected(compression,b'gdxrs'.hex(),b'gdxrs'.hex(),large));raise ValueError('Rust string write / native/Rust read mismatch')
                cases.append(dict(features=feature,compression=compression,large=large,statistics=stats,sha256=hashlib.sha256(path.read_bytes()).hexdigest(),recipe_sha256=hashlib.sha256(recipe_path.read_bytes()).hexdigest(),both_directions_complete_match=True))
    return dict(cases=cases,native_isolation=isolation_policy(),scope='two-pass exact-byte labels and original IDs; fixed semantics plus 8192 generated 63-byte labels; no M4 OS-scale claim')
