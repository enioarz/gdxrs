"""Task-028 fixed numeric/acronym recipes; <=3 labels/rows and two acronyms."""
import hashlib
import json
import struct
import subprocess
import sys
from pathlib import Path
import fixtures
import oracle
from oracle_limits import isolation_policy
ROOT=Path(__file__).resolve().parents[1]
SPECIALS=['s:undef','s:na','s:pinf','s:minf','s:eps']
def hex_bits(v):return struct.pack('>d',v).hex()
def rows(bit_map):
    return [SPECIALS,(['f:'+hex_bits(8.+f) for f in range(5)] if bit_map else ['f:3ff4000000000000','f:0000000000000000','s:na','s:pinf','s:minf']),['a:7','a:9','f:4045000000000000','s:na','s:eps']]
def expected(compression,producer,audit,bit_map):
    lines=['GDXRS-ORACLE\t1',f'F\t7\t{compression}\t1\t3\t{producer}\t{audit}','U\t1\t61','U\t2\t62','U\t3\t63','T\t0\t0\t','A\t1\t7\t416c706861\t72656e616d6564','A\t2\t9\t42657461\t7365636f6e64','S\t0\t2a\t1\t0\t3\t0\t556e697665727365']
    lines += [f'R\t0\t{n}\tf:0000000000000000' for n in range(1,4)]+['E\t0\t3','S\t1\t56\t1\t2\t3\t5\t','D\t1\t1\t0:2a']
    lines += [f'R\t1\t{n}\t'+','.join(v) for n,v in enumerate(rows(bit_map),1)]+['E\t1\t3']
    return '\n'.join(lines)+'\n'
def projected(bit_map,mode):
    sentinels=[hex_bits((f+1)*1e300) for f in range(5)] if mode in ['identity','reset'] else ['7ff8000000001234']*5 if mode=='loss' else [hex_bits(-201.-f) for f in range(5)]
    out=['GDXRS-VALUE-PROJECTION\t1']
    for n,row in enumerate(rows(bit_map),1):
        values=[]
        for value in row:
            if value.startswith('f:'):values.append(value[2:])
            elif value.startswith('s:'):values.append(sentinels[SPECIALS.index(value)])
            elif mode=='loss':values.append(hex_bits(-102.))
            else:
                original=int(value[2:]);target=original if mode=='identity' else 70 if original==7 or mode=='collision' else 100
                values.append(hex_bits(target*1e301))
        out.append(f'R\t{n}\t'+','.join(values))
    out.append('N\t'+str(0 if mode=='identity' else 100 if mode=='loss' else 71 if mode=='collision' else 101))
    return '\n'.join(out)+'\n'
def build(run,area):
    native=json.loads((oracle.AREA/'current.json').read_text());library=Path(native['executable']).parent.parent/'gdx/libgdx-static.a';directory=area/'values-driver'
    run([oracle.cmake(),'-S',ROOT/'tools/values','-B',directory,'-DCMAKE_BUILD_TYPE=Release',f'-DGDX_ROOT={oracle.AREA/"gdx"}',f'-DGDX_LIBRARY={library}'],capture=True)
    run([oracle.cmake(),'--build',directory,'--parallel','2'],capture=True,timeout=180)
    executable=directory/'gdxrs-values';digest=hashlib.sha256(oracle.source_fingerprint().encode())
    for path in ['tools/values/CMakeLists.txt','tools/values/driver.cpp']:digest.update((ROOT/path).read_bytes())
    return executable,dict(source_fingerprint=digest.hexdigest(),executable_sha256=oracle.file_digest(executable),frozen_oracle=oracle.file_digest(oracle.executable()))
def check(run,area,executable):
    helper,identity=build(run,area);cases=[]
    probe=run([sys.executable,ROOT/'tools/oracle_limits.py',helper,'probes'],capture=True,timeout=5).stdout
    if probe!='write_duplicate_rejected\t1\nread_duplicate_allowed\t1\nreset\t1\nacronym_overflow_is_infinity\t1\n':raise ValueError('native sentinel validation/reset/overflow expectation changed')
    for bit_map in [False,True]:
        for compression in [0,1]:
            native_path=area/f'values-native-{bit_map}-{compression}.gdx'
            run([sys.executable,ROOT/'tools/oracle_limits.py',helper,'author',native_path,str(compression),'bits' if bit_map else 'normal'],capture=True,timeout=5)
            native_dump=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',native_path],capture=True,timeout=5).stdout
            if fixtures.normalize_dump(native_dump)!=expected(compression,b'gdxrs-values'.hex(),'<creation-system>',bit_map):raise ValueError('native custom sentinel/IEEE/acronym metadata expectation changed')
            collision=run([sys.executable,ROOT/'tools/oracle_limits.py',helper,'project',native_path,'collision'],capture=True,timeout=5).stdout
            if collision!=projected(bit_map,'collision'):raise ValueError('native automatic acronym collision probe changed')
            for feature in [[],['--all-features']]+([['--no-default-features']] if not compression else []):
                rust_dump=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',native_path],capture=True).stdout
                if rust_dump!=expected(compression,b'gdxrs-values'.hex(),'<creation-system>',bit_map):raise ValueError('native write/Rust typed read mismatch')
                rust_path=area/f'values-rust-{len(cases)}.gdx'
                created=run(['cargo','run','--locked','--quiet',*feature,'--example','create_values','--',rust_path,area,*(['zlib'] if compression else []),*(['bits'] if bit_map else [])],capture=True)
                stats=json.loads(created.stdout)
                if not 0<stats['peak_root']<=1048576 or not 0<stats['peak_handles']<=6:raise ValueError('value author exceeded root/handles')
                if rust_path.stat().st_size>1048576:raise ValueError('value recipe exceeds fixed small-artifact bound')
                native_dump=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',rust_path],capture=True,timeout=5).stdout
                rust_dump=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',rust_path],capture=True).stdout
                if native_dump!=expected(compression,b'gdxrs'.hex(),b'gdxrs'.hex(),bit_map) or rust_dump!=expected(compression,b'gdxrs'.hex(),'<creation-system>',bit_map):raise ValueError('Rust write/native typed metadata/record expectation mismatch')
                streams=[]
                for source in [native_path,rust_path]:
                    for mode in ['identity','read','loss','reset']:
                        native_projection=run([sys.executable,ROOT/'tools/oracle_limits.py',helper,'project',source,mode],capture=True,timeout=5).stdout
                        rust=run(['cargo','run','--locked','--quiet',*feature,'--example','project_values','--',source,area,mode],capture=True)
                        if native_projection!=projected(bit_map,mode) or rust.stdout!=projected(bit_map,mode):raise ValueError('custom value projection/remapping/reset complete stream mismatch')
                        resources=json.loads(rust.stderr)
                        if not 0<resources['peak_root']<=1048576 or not 0<resources['peak_handles']<=6:raise ValueError('value projection root/handles exceeded')
                        streams.append(dict(source='native' if source==native_path else 'rust',mode=mode,sha256=hashlib.sha256(rust.stdout.encode()).hexdigest(),statistics=resources))
                # Rust deliberately rejects native's ambiguous automatic target collision.
                rejected=subprocess.run(list(map(str,['cargo','run','--locked','--quiet',*feature,'--example','project_values','--',native_path,area,'collision'])),cwd=ROOT,capture_output=True,text=True,timeout=180)
                if rejected.returncode==0 or 'acronym mapping collision' not in rejected.stderr:raise ValueError('Rust automatic acronym collision must be terminal')
                cases.append(dict(features=feature,compression=compression,bitwise_sentinels=bit_map,statistics=stats,sha256=oracle.file_digest(rust_path),projections=streams,native_collision_sha256=hashlib.sha256(collision.encode()).hexdigest(),rust_collision_rejects=True,both_directions_complete_match=True))
    return dict(cases=cases,driver=identity,native_probes=probe,native_isolation=isolation_policy(),scope='exact custom sentinel bits, explicit IEEE normalization, original typed view, read/internal NA precedence, explicit/auto/identity acronym projections, reset, strict collision and finite-encoding bounds')
