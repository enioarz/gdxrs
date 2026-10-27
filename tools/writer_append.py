"""Task-030 fixed append recipe and frozen-corpus rewrites (<=4 MiB text per dump)."""
import hashlib
import json
import shutil
import sys
from pathlib import Path
import fixtures
import oracle
import writer_metadata_ops
from oracle_limits import isolation_policy
ROOT=Path(__file__).resolve().parents[1]

def expected(compression, producer):
    # Source recipe is the independently specified task-029 catalog, not a dump
    # of either implementation. Extend its explicit IDs and rows here.
    lines=writer_metadata_ops.expected(compression,producer,'<creation-system>').splitlines()
    fields=lines[1].split('\t');fields[3:5]=['8','5'];lines[1]='\t'.join(fields)
    at=next(n for n,line in enumerate(lines) if line.startswith('T\t0\t'))
    lines.insert(at,'U\t5\t'+b'omega'.hex())
    at=next(n for n,line in enumerate(lines) if line.startswith('S\t0\t'))
    lines[at:at]=['T\t3\t0\t'+b'new text'.hex(),'A\t1\t9\t'+b'AddedAcro'.hex()+'\t'+b'appended'.hex()]
    at=next(n for n,line in enumerate(lines) if line.startswith('S\t0\t'))
    lines[at]='S\t0\t2a\t1\t0\t5\t0\t556e697665727365'
    at=lines.index('E\t0\t4');lines[at:at+1]=['R\t0\t5\tf:0000000000000000','E\t0\t5']
    lines += ['S\t6\t'+b'NewSet'.hex()+'\t1\t0\t1\t0\t','D\t6\t1\t0:2a','R\t6\t5\tf:4008000000000000','E\t6\t1',
      'S\t7\t'+b'NewParam'.hex()+'\t2\t1\t1\t0\t','D\t7\t3\t2:4a\t6:'+b'NewSet'.hex(),'C\t7\t1\t'+b'appended comment'.hex(),'R\t7\t1,5\ta:9','E\t7\t1',
      'S\t8\t'+b'NewAlias'.hex()+'\t1\t4\t0\t6\t'+b'Aliased with NewSet'.hex(),'D\t8\t1\t0:2a','R\t8\t5\tf:4008000000000000','E\t8\t1']
    return '\n'.join(lines)+'\n'

def normalized(text):
    lines=text.splitlines()
    if len(lines)<2 or lines[0]!='GDXRS-ORACLE\t1':raise ValueError('bad append dump header')
    fields=lines[1].split('\t')
    if len(fields)!=7:raise ValueError('bad append file information')
    # Creation-system bytes are checked separately without normalization.
    fields[6]='<creation-system>';lines[1]='\t'.join(fields)
    return '\n'.join(lines)+'\n'

def header(path):
    with path.open('rb') as f:data=f.read(1024)
    if data[:17] not in [bytes.fromhex('023412047856341208182d4454fb210940'),bytes.fromhex('021234041234567808400921fb54442d18')]:raise ValueError('bad fixed source header')
    at=18
    size=data[at];at+=1+size+8
    strings=[]
    for _ in range(2):
        size=data[at];at+=1;strings.append(data[at:at+size]);at+=size
    return strings

def build(run,area):
    native=json.loads((oracle.AREA/'current.json').read_text());library=Path(native['executable']).parent.parent/'gdx/libgdx-static.a';directory=area/'append-driver'
    run([oracle.cmake(),'-S',ROOT/'tools/append','-B',directory,'-DCMAKE_BUILD_TYPE=Release',f'-DGDX_ROOT={oracle.AREA/"gdx"}',f'-DGDX_LIBRARY={library}'],capture=True)
    run([oracle.cmake(),'--build',directory,'--parallel','2'],capture=True,timeout=180)
    digest=hashlib.sha256(oracle.source_fingerprint().encode())
    for p in ['tools/append/CMakeLists.txt','tools/append/driver.cpp']:digest.update((ROOT/p).read_bytes())
    executable=directory/'gdxrs-append'
    return executable,dict(source_fingerprint=digest.hexdigest(),executable_sha256=oracle.file_digest(executable))

def check(run,area,executable):
    helper,identity=build(run,area);author,_=writer_metadata_ops.build(run,area)
    cases=[];native_cases=[];corpus=[]
    def compare(path,wanted,feature):
        native=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',path],capture=True,timeout=5).stdout
        rust=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',path],capture=True).stdout
        fixtures.compare(wanted,normalized(native),'native append stream')
        fixtures.compare(wanted,normalized(rust),'Rust append stream')
    for input_compression in [0,1]:
        for origin in ['native','rust']:
            source=area/f'append-source-{origin}-{input_compression}.gdx'
            producer=(b'gdxrs-metadata-ops' if origin=='native' else b'gdxrs').hex()
            if origin=='native':run([sys.executable,ROOT/'tools/oracle_limits.py',author,'author',source,str(input_compression)],capture=True,timeout=5)
            else:run(['cargo','run','--locked','--quiet','--example','create_metadata_ops','--',source,area,*(['zlib'] if input_compression else [])],capture=True)
            before=oracle.file_digest(source);original_header=header(source)
            compare(source,writer_metadata_ops.expected(input_compression,producer,'<creation-system>'),[])
            reference=area/f'append-native-{origin}-{input_compression}.gdx';shutil.copyfile(source,reference)
            run([sys.executable,ROOT/'tools/oracle_limits.py',helper,'append',reference],capture=True,timeout=5)
            compare(reference,expected(input_compression,producer),[])
            native_cases.append(dict(origin=origin,compression=input_compression,sha256=oracle.file_digest(reference),complete_native_and_rust_stream_matches=True))
            for output_compression in [0,1]:
                for feature in [[],['--all-features']]+([['--no-default-features']] if not input_compression and not output_compression else []):
                    for policy in ['new','replace']:
                        dest=area/f'append-result-{len(cases)}.gdx';working=source
                        if policy=='replace':shutil.copyfile(source,dest);working=dest
                        made=run(['cargo','run','--locked','--quiet',*feature,'--example','append_gdx','--',working,dest,area,'zlib' if output_compression else 'raw',policy,'add'],capture=True)
                        stats=json.loads(made.stdout)
                        if not stats['published'] or stats['rewritten_records']!=6 or not 0<stats['peak_root']<=4*1024*1024 or stats['admitted_transaction_bytes']!=12*1024*1024 or stats['admitted_open_files']!=10 or stats['peak_scratch_handles']>8:raise ValueError('append root/transaction/disk/handle/count bounds changed')
                        if header(dest)!=original_header:raise ValueError('append changed prior audit/producer bytes')
                        compare(dest,expected(output_compression,producer),feature)
                        cases.append(dict(origin=origin,input_compression=input_compression,output_compression=output_compression,features=feature,policy=policy,statistics=stats,sha256=oracle.file_digest(dest),complete_native_and_rust_stream_matches=True))
            if oracle.file_digest(source)!=before:raise ValueError('append changed an unowned original source')
    manifest=json.loads((ROOT/'tests/fixtures/manifest.json').read_text())
    for item in manifest['fixtures']:
        source=ROOT/'tests/fixtures'/item['path'];golden=ROOT/'tests/fixtures'/item['expectation']
        if oracle.file_digest(source)!=item['sha256'] or oracle.file_digest(golden)!=item['expectation_sha256']:raise ValueError('frozen append input/golden digest mismatch')
        wanted=golden.read_text()
        if len(wanted.encode())>4*1024*1024:raise ValueError('frozen dump exceeds fixed tooling bound')
        fields=wanted.splitlines()[1].split('\t');fields[2]='0';lines=wanted.splitlines();lines[1]='\t'.join(fields);wanted='\n'.join(lines)+'\n'
        dest=area/f'append-corpus-{item["id"]}.gdx'
        made=run(['cargo','run','--locked','--quiet','--example','append_gdx','--',source,dest,area,'raw','new','only'],capture=True)
        compare(dest,wanted,[])
        if header(dest)!=header(source) or oracle.file_digest(source)!=item['sha256']:raise ValueError('frozen append header/source changed')
        corpus.append(dict(id=item['id'],sha256=oracle.file_digest(dest),statistics=json.loads(made.stdout),complete_frozen_expectation_matches=True))
    return dict(cases=cases,native_append_cases=native_cases,frozen_rewrites=corpus,driver=identity,native_isolation=isolation_policy(),scope='checked O(file-size) rewrite; exact prior UEL/symbol IDs, audit/producer, metadata and physical records; explicit new or source replacement; native in-place append is only an interoperability reference')
