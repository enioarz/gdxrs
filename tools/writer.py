"""Bounded development interoperability for the current M3 writer foundation."""
import hashlib
import json
import os
import re
from pathlib import Path
import struct
import sys
import uuid
import zlib

import oracle
import fixtures
from oracle_limits import isolation_policy

ROOT = Path(__file__).resolve().parents[1]
RUST_PRODUCER = b'gdxrs'.hex()
NATIVE_PRODUCER = b'gdxrs-fixtures'.hex()
EMPTY_TAIL = 'T\t0\t0\t\nS\t0\t2a\t1\t0\t0\t0\t556e697665727365\nE\t0\t0\n'


def expected(compression, producer, audit):
    return f'GDXRS-ORACLE\t1\nF\t7\t{compression}\t0\t0\t{producer}\t{audit}\n'+EMPTY_TAIL


def check(run):
    oracle.build(run)
    executable = oracle.executable()
    area = ROOT/'.work/writer'/uuid.uuid4().hex
    area.mkdir(parents=True)
    cases = []
    for feature in [[], ['--no-default-features'], ['--all-features']]:
        path = area/f'rust-empty-{len(cases)}.gdx'
        created = run(['cargo','run','--locked','--quiet',*feature,'--example','create_empty','--',path],capture=True)
        if json.loads(created.stdout) != dict(output_bytes=197,bytes_written=249,published=True):
            raise ValueError('empty writer completion/statistics mismatch')
        if path.stat().st_size != 197:
            raise ValueError('writer output is not the bounded 197-byte fixture')
        with path.open('rb') as stream:
            data = stream.read(198)  # Bounded even if a broken output grows concurrently.
        if len(data) != 197 or struct.unpack_from('<6q',data,50) != (126,163,144,179,126,0):
            raise ValueError('empty writer size/major offsets differ from independent layout')
        native = run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',path],capture=True,timeout=5).stdout
        if native != expected(0,RUST_PRODUCER,RUST_PRODUCER):
            raise ValueError('native metadata/record stream differs from independent empty expectation')
        rust = run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',path],capture=True).stdout
        if rust != expected(0,RUST_PRODUCER,'<creation-system>'):
            raise ValueError('Rust metadata/record stream differs from independent empty expectation')
        cases.append(dict(direction='Rust writes → native reads',features=feature,artifact=str(path.relative_to(ROOT)),bytes=len(data),sha256=oracle.file_digest(path),native_complete_stream_matches=True,rust_complete_stream_matches=True))
    for feature in [[], ['--all-features']]:
        path = area/f'rust-zlib-empty-{len(cases)}.gdx'
        created = run(['cargo','run','--locked','--quiet',*feature,'--example','create_empty','--',path,'zlib'],capture=True)
        stats = json.loads(created.stdout)
        if not (126 < path.stat().st_size <= 209):
            raise ValueError('compressed empty writer exceeds independent bound')
        with path.open('rb') as stream:
            data = stream.read(210)
        if stats != dict(output_bytes=len(data),bytes_written=len(data)+52,published=True):
            raise ValueError('compressed writer statistics differ')
        offsets = struct.unpack_from('<6q',data,50)
        if offsets[0] != 126 or offsets[4:] != (126,0) or struct.unpack_from('<i',data,30)[0] != 1:
            raise ValueError('compressed empty header offsets/flag differ')
        boundaries = [offsets[0],offsets[2],offsets[1],offsets[3],len(data)]
        chunks = [b'\x06_SYMB_\0\0\0\0\x06_SYMB_',b'\x06_SETT_\x01\0\0\0\0\x06_SETT_',b'\x05_UEL_\0\0\0\0\x05_UEL_',b'\x06_ACRO_\0\0\0\0\x06_ACRO_']
        for start,end,wanted in zip(boundaries,boundaries[1:],chunks):
            actual,_ = decode_blocks(data[start:end])
            if actual != wanted:
                raise ValueError('independently decoded compressed empty section differs')
        native = run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',path],capture=True,timeout=5).stdout
        rust = run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',path],capture=True).stdout
        if native != expected(1,RUST_PRODUCER,RUST_PRODUCER) or rust != expected(1,RUST_PRODUCER,'<creation-system>'):
            raise ValueError('compressed empty complete semantic stream differs')
        cases.append(dict(direction='Rust writes → native reads',compression=1,features=feature,artifact=str(path.relative_to(ROOT)),bytes=len(data),sha256=oracle.file_digest(path),native_complete_stream_matches=True,rust_complete_stream_matches=True))
    recipe = area/'empty.recipe.tsv'
    recipe.write_text('# No UELs or symbols; upstream initializes empty set-text ID 0.\n')
    for compression in [0,1]:
        path = area/f'native-empty-{compression}.gdx'
        run([executable,'recipe',path,recipe,str(compression)])
        native = run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',path],capture=True,timeout=5).stdout
        if fixtures.normalize_dump(native) != expected(compression,NATIVE_PRODUCER,'<creation-system>'):
            raise ValueError('native empty recipe differs from source-backed independent expectation')
        features = [[], ['--all-features']]
        if compression == 0:
            features.append(['--no-default-features'])
        for feature in features:
            actual = run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',path],capture=True).stdout
            if actual != expected(compression,NATIVE_PRODUCER,'<creation-system>'):
                raise ValueError('Rust failed complete upstream-written empty metadata/record stream')
        cases.append(dict(direction='Native writes → Rust reads',compression=compression,features=features,artifact=str(path.relative_to(ROOT)),sha256=oracle.file_digest(path),complete_stream_matches=True))
    codec = codec_check(run, area)
    metadata = metadata_check(run, area, executable)
    import writer_records
    records = writer_records.check(run,area,executable,decode_blocks)
    import writer_full
    full = writer_full.check(run,area,executable,decode_blocks)
    import writer_strings
    strings = writer_strings.check(run,area,executable)
    return dict(strings=strings,full=full,scope='tasks 018–022 metadata, sorted records and bounded section codec',records=records,metadata=metadata,codec=codec,cases=cases,root=str(area.relative_to(ROOT)),native_executable_sha256=oracle.file_digest(executable),native_read_isolation=isolation_policy(),pinned_sources=json.loads((ROOT/'tools/oracle/sources.json').read_text()))


def decode_blocks(data):
    """Independent Python-zlib framing/checksum/full-consumption verification."""
    output = bytearray()
    blocks = []
    offset = 0
    while offset < len(data):
        if len(data)-offset < 3:
            raise ValueError('truncated block header')
        tag,n = data[offset],int.from_bytes(data[offset+1:offset+3],'big')
        if tag not in [0,1] or not 1 <= n <= 32768 or offset+3+n > len(data):
            raise ValueError('invalid output framing')
        payload = data[offset+3:offset+3+n]
        if tag:
            decoder = zlib.decompressobj()
            decoded = decoder.decompress(payload,32769)
            if not decoder.eof or decoder.unused_data or decoder.unconsumed_tail or not n < len(decoded) <= 32768:
                raise ValueError('non-independent/checksum/size/useful-compression failure')
        else:
            decoded = payload
        blocks.append(dict(tag=tag,encoded=n,decoded=len(decoded)))
        output.extend(decoded)
        if len(output) > 131072:
            raise ValueError('development decoded fixture bound exceeded')
        offset += n+3
    return bytes(output),blocks


def bounded_read(path):
    if path.stat().st_size > 131072:
        raise ValueError('development fixture size bound exceeded')
    with path.open('rb') as stream:
        data = stream.read(131073)
    if len(data) > 131072:
        raise ValueError('development fixture grew beyond bound')
    return data


def codec_check(run, area):
    current = json.loads((ROOT/'.work/oracle/current.json').read_text())
    sources = ['tools/writer-codec/driver.cpp','tools/writer-codec/CMakeLists.txt']
    digest = hashlib.sha256(current['identity'].encode())
    for source in sources:
        digest.update(source.encode()); digest.update((ROOT/source).read_bytes())
    identity = digest.hexdigest()
    build = ROOT/'.work/writer-codec/builds'/identity
    run([oracle.cmake(),'-S',ROOT/'tools/writer-codec','-B',build,'-DCMAKE_BUILD_TYPE=Release',f'-DGDX_ROOT={ROOT/".work/oracle/gdx"}',f'-DGDX_LIBRARY={Path(current["executable"]).parents[1]/"gdx/libgdx-static.a"}'])
    run([oracle.cmake(),'--build',build,'--parallel','2'])
    executable = build/'gdxrs-writer-codec'
    probes = area/'codec'
    probes.mkdir()
    env = dict(os.environ, GDXRS_CODEC_PROBES=str(probes))
    env.pop('GDXRS_CODEC_VERIFY_NATIVE',None)
    command = ['cargo','test','--locked','--lib','write::codec_tests::native_codec_probes','--','--exact','--nocapture']
    generated = run(command,env=env,capture=True)
    if '1 passed; 0 failed' not in generated.stdout:
        raise ValueError('native codec probe generator did not execute')
    state = 0x12345678
    noise = bytearray()
    for _ in range(32768):
        state ^= (state << 13) & 0xffffffff; state ^= state >> 17; state ^= (state << 5) & 0xffffffff
        noise.append(state & 255)
    recipes = dict(empty=b'',one=b'x',small=b'abc',full=b'x'*32768,final=b'x'*32769,repeat=b'x'*65537,noise=bytes(noise),boundary=b'a'*32766+b'\x78\x56\x34\x12\xff'+b'z'*255+b'q'*32508+b'\x08boundary')
    cases = []
    for name,wanted in recipes.items():
        raw = probes/f'{name}.raw'; encoded = probes/f'{name}.blocks'
        if bounded_read(raw) != wanted:
            raise ValueError('Rust test input differs from independent Python recipe')
        data = bounded_read(encoded)
        decoded,blocks = decode_blocks(data)
        if decoded != wanted or len(blocks) != (len(wanted)+32767)//32768:
            raise ValueError('Rust blocks differ from independent Python expectation')
        native_decoded = probes/f'{name}.decoded'
        native_encoded = probes/f'{name}.native'
        for mode,input_path,output in [('decode',encoded,native_decoded),('encode',raw,native_encoded)]:
            actual = run([sys.executable,ROOT/'tools/oracle_limits.py',executable,mode,input_path,output],capture=True,timeout=5)
            if int(actual.stdout.strip()) != len(wanted):
                raise ValueError('native buffered stream byte count differs')
        if bounded_read(native_decoded) != wanted or decode_blocks(bounded_read(native_encoded))[0] != wanted:
            raise ValueError('native buffered stream differs from independent recipe')
        cases.append(dict(name=name,decoded_bytes=len(wanted),rust_blocks=blocks,raw_sha256=hashlib.sha256(wanted).hexdigest(),rust_sha256=hashlib.sha256(data).hexdigest(),native_sha256=oracle.file_digest(native_encoded)))
    env['GDXRS_CODEC_VERIFY_NATIVE']='1'
    for features in [[],['--all-features']]:
        actual = run(['cargo','test','--locked',*features,'--lib','write::codec_tests::native_codec_probes','--','--exact','--nocapture'],env=env,capture=True)
        if '1 passed; 0 failed' not in actual.stdout:
            raise ValueError('Rust native-stream comparison did not execute')
    return dict(cases=cases,helper_build_identity=identity,helper_sha256=oracle.file_digest(executable),oracle_identity=current['identity'],native_isolation=isolation_policy(),rust_native_read_features=['default','all'],visible_state_bytes=int(re.search(r'encoder_visible_state_bytes=(\d+)',generated.stderr).group(1)),private_state_reservation_bytes=320*1024)


def metadata_expected(compression, audit):
    """Independent recipe for create_metadata, including native scalar defaults."""
    lines=['GDXRS-ORACLE\t1',f'F\t7\t{compression}\t8\t7\t{RUST_PRODUCER}\t{audit}']
    labels=[b'z',b'a',b'A',b'\x80label',b' space ',b'',b"single'"]
    for n,label in enumerate(labels,1): lines.append(f'U\t{n}\t{label.hex()}')
    lines += ['T\t0\t0\t','T\t1\t0\t'+b'note'.hex(),'A\t1\t7\t'+b'Acro'.hex()+'\t'+b'acronym'.hex()]
    zero='f:0000000000000000';one='f:3ff0000000000000'
    symbols=[(b'*',1,0,7,0,b'Universe'),(b'Alias',1,4,0,2,b''),(b'I',1,0,0,0,b'domain set'),(b'Scalar',0,1,1,0,b'scalar'),(b'P',2,1,0,0,b''),(b'V',1,2,0,5,b''),(b'E',0,3,1,53,b''),(b'All',1,4,0,0,b''),(b'PAlias',1,1,0,0,b'')]
    for n,(name,dimension,kind,count,user,description) in enumerate(symbols):
        lines.append(f'S\t{n}\t{name.hex()}\t{dimension}\t{kind}\t{count}\t{user}\t{description.hex()}')
        if n:
            mode=3 if n in [4,8] else 2 if n==5 else 1
            domains=['1:'+b'Alias'.hex()] if n==8 else ['2:'+b'I'.hex(),'0:2a'] if n==4 else ['0:'+b'future'.hex()] if n==5 else ['0:2a']*dimension
            lines.append(f'D\t{n}\t{mode}'+''.join('\t'+d for d in domains))
            if n==2:
                for c,text in enumerate([b'first',b'second'],1): lines.append(f'C\t{n}\t{c}\t{text.hex()}')
        if n in [0,7]:
            for id in range(1,8): lines.append(f'R\t{n}\t{id}\t{zero}')
            rows=7
        elif n in [3,6]:
            values=zero if n==3 else ','.join([zero]*4+[one])
            lines.append(f'R\t{n}\t\t{values}');rows=1
        else: rows=0
        lines.append(f'E\t{n}\t{rows}')
    return '\n'.join(lines)+'\n'


def metadata_check(run, area, executable):
    cases=[]
    for compression in [0,1]:
        features=[[],['--all-features']]
        if not compression: features.append(['--no-default-features'])
        for feature in features:
            path=area/f'metadata-{compression}-{len(cases)}.gdx'
            run(['cargo','run','--locked','--quiet',*feature,'--example','create_metadata','--',path,area,*(['zlib'] if compression else [])],capture=True)
            if len(bounded_read(path))>4096: raise ValueError('metadata fixture exceeds independent small bound')
            native=run([sys.executable,ROOT/'tools/oracle_limits.py',executable,'dump',path],capture=True,timeout=5).stdout
            rust=run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',path],capture=True).stdout
            if native!=metadata_expected(compression,RUST_PRODUCER) or rust!=metadata_expected(compression,'<creation-system>'):
                raise ValueError('metadata/symbol/empty-scalar streams differ from independent recipe')
            cases.append(dict(features=feature,compression=compression,bytes=path.stat().st_size,sha256=oracle.file_digest(path),complete_native_and_rust_stream_matches=True))
    return dict(scope='task 020 registered metadata and empty symbols only',cases=cases)
