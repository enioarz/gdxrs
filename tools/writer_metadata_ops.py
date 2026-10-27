"""Task-029 fixed metadata/UEL mutation recipes; <=5 symbols, four UELs, three texts."""
import hashlib
import json
import sys
from pathlib import Path
import fixtures
import oracle
from oracle_limits import isolation_policy
ROOT=Path(__file__).resolve().parents[1]
Z='f:0000000000000000';ONE='f:3ff0000000000000'
LABELS=[b'alpha-long-label',b'beta-final',b'gamma',b'Z'*63]
TEXTS=[b'',b'element text',b'spare']
PROBES=['rename_missing_rejected','rename_duplicate_rejected','rename_invalid_rejected','native_uel_case_folded','symbol_case_folded','invalid_uel_rejected','invalid_text_node_rejected','zero_node_slot_supported','nonzero_node_immutable','sparse_node_preserved']
def expected(compression,producer,audit):
    specs=[(b'I',1,0,0,b'',1,['0:2a'],[b'after I'],[([1],[ONE]),([2],[Z])]),
      (b'J',1,4,1,b'Aliased with I',1,['0:2a'],[],[([1],[ONE]),([2],[Z])]),
      (b'K',1,0,0,b'',1,['0:2a'],[],[([3],[Z])]),
      (b'P',2,1,0,b'',2,['2:53686f776e416761696e','1:2a'],[b'late P'],[([1,2],['s:eps']),([2,1],['f:3ff4000000000000'])]),
      (b'R',0,1,0,b'',1,[],[],[([],['f:4045000000000000'])])]
    lines=['GDXRS-ORACLE\t1',f'F\t7\t{compression}\t5\t4\t{producer}\t{audit}']+[f'U\t{n}\t'+label.hex() for n,label in enumerate(LABELS,1)]+[f'T\t{n}\t0\t'+text.hex() for n,text in enumerate(TEXTS)]
    lines += ['S\t0\t2a\t1\t0\t4\t0\t556e697665727365']+[f'R\t0\t{n}\t{Z}' for n in range(1,5)]+['E\t0\t4']
    for n,(name,dim,kind,user,description,mode,domains,comments,rows) in enumerate(specs,1):
        lines += [f'S\t{n}\t{name.hex()}\t{dim}\t{kind}\t{0 if kind==4 else len(rows)}\t{user}\t{description.hex()}',f'D\t{n}\t{mode}'+''.join('\t'+domain for domain in domains)]
        lines += [f'C\t{n}\t{c}\t'+text.hex() for c,text in enumerate(comments,1)]
        lines += [f'R\t{n}\t'+','.join(map(str,keys))+'\t'+','.join(values) for keys,values in rows]+[f'E\t{n}\t{len(rows)}']
    return '\n'.join(lines)+'\n'
def context(view):
    labels=[b'session-label',LABELS[1],b'session-gamma',LABELS[3]] if view else LABELS
    lines=['GDXRS-METADATA-CONTEXT\t1','M\t4\t9']+[f'U\t{n}\t{mapped}\t'+label.hex() for n,(label,mapped) in enumerate(zip(labels,[9,0,3,-1]),1)]+['G\t9\t'+labels[0].hex(),'F\t1\t9']
    lines += [f'T\t{n}\t{node}\t'+text.hex() for n,(text,node) in enumerate(zip(TEXTS,[2147483647,-2147483648,0] if view else [0,0,0]))]
    lines += [f'H\t{n}\t{int(n==1)}' for n in range(1,6)]
    maximum=13 if view else 16
    lines += [f'L\t63\t1\t{maximum}\t{maximum},{maximum}','D\t2\t0','D\t1\t9','E\t2']
    return '\n'.join(lines)+'\n'
def build(run,area):
    native=json.loads((oracle.AREA/'current.json').read_text());library=Path(native['executable']).parent.parent/'gdx/libgdx-static.a';directory=area/'metadata-ops-driver'
    run([oracle.cmake(),'-S',ROOT/'tools/metadata_ops','-B',directory,'-DCMAKE_BUILD_TYPE=Release',f'-DGDX_ROOT={oracle.AREA/"gdx"}',f'-DGDX_LIBRARY={library}'],capture=True)
    run([oracle.cmake(),'--build',directory,'--parallel','2'],capture=True,timeout=180)
    executable=directory/'gdxrs-metadata-ops';digest=hashlib.sha256(oracle.source_fingerprint().encode())
    for path in ['tools/metadata_ops/CMakeLists.txt','tools/metadata_ops/driver.cpp']:digest.update((ROOT/path).read_bytes())
    return executable,dict(source_fingerprint=digest.hexdigest(),executable_sha256=oracle.file_digest(executable),frozen_oracle=oracle.file_digest(oracle.executable()))
def check(run,area,executable):
    helper,identity=build(run,area);cases=[];probes=[]
    for compression in [0,1]:
        native_path=area/f'metadata-ops-native-{compression}.gdx'
        created=run([sys.executable,ROOT/'tools/oracle_limits.py',helper,'author',native_path,str(compression)],capture=True,timeout=5).stdout
        if created!='writer_node\t-2147483648\n':raise ValueError('native writer set-text node/overwrite expectation changed')
        probe=run([sys.executable,ROOT/'tools/oracle_limits.py',helper,'probes',native_path],capture=True,timeout=5).stdout
        if probe!=''.join(p+'\t1\n' for p in PROBES):raise ValueError('native rename/lookup/case/node invariant probe changed')
        probes.append(dict(compression=compression,output=probe))
        raw=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',native_path],capture=True,timeout=5).stdout
        if fixtures.normalize_dump(raw)!=expected(compression,b'gdxrs-metadata-ops'.hex(),'<creation-system>'):raise ValueError('native complete UEL/metadata/domain/record expectation changed')
        for feature in [[],['--all-features']]+([['--no-default-features']] if not compression else []):
            raw=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',native_path],capture=True).stdout
            if raw!=expected(compression,b'gdxrs-metadata-ops'.hex(),'<creation-system>'):raise ValueError('native write/Rust read metadata operations mismatch')
            rust_path=area/f'metadata-ops-rust-{len(cases)}.gdx';made=run(['cargo','run','--locked','--quiet',*feature,'--example','create_metadata_ops','--',rust_path,area,*(['zlib'] if compression else [])],capture=True);stats=json.loads(made.stdout)
            if not 0<stats['peak_root']<=1048576 or not 0<stats['peak_handles']<=8:raise ValueError('metadata author root/handles exceeded')
            if rust_path.stat().st_size>1048576:raise ValueError('metadata recipe exceeds fixed small-artifact bound')
            native=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',rust_path],capture=True,timeout=5).stdout
            rust=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',rust_path],capture=True).stdout
            if native!=expected(compression,b'gdxrs'.hex(),b'gdxrs'.hex()) or rust!=expected(compression,b'gdxrs'.hex(),'<creation-system>'):raise ValueError('Rust write/native complete metadata/alias/domain/record mismatch')
            streams=[]
            for source in [native_path,rust_path]:
                before=oracle.file_digest(source)
                for view in [False,True]:
                    native=run([sys.executable,ROOT/'tools/oracle_limits.py',helper,'context',source,'view' if view else 'base'],capture=True,timeout=5).stdout
                    rust=run(['cargo','run','--locked','--quiet',*feature,'--example','metadata_context','--',source,area,'view' if view else 'base'],capture=True)
                    if native!=context(view) or rust.stdout!=context(view):raise ValueError('complete metadata session/mapped labels/nodes/flags/lengths/domain callback mismatch')
                    resources=json.loads(rust.stderr)
                    if not 0<resources['peak_root']<=1048576 or not 0<resources['peak_handles']<=8:raise ValueError('metadata view root/handles exceeded')
                    streams.append(dict(source='native' if source==native_path else 'rust',view=view,sha256=hashlib.sha256(rust.stdout.encode()).hexdigest(),statistics=resources))
                if oracle.file_digest(source)!=before:raise ValueError('read-session metadata operations modified the source file')
            cases.append(dict(features=feature,compression=compression,statistics=stats,sha256=oracle.file_digest(rust_path),contexts=streams,both_directions_complete_match=True))
    return dict(cases=cases,native_probes=probes,driver=identity,native_isolation=isolation_policy(),scope='ID-stable post-record renames, mapped label views, signed ephemeral set-text nodes, strict and relaxed domains together, late comments, set-text flags, lengths and mapped-order domain callbacks; exact source bytes retained')
