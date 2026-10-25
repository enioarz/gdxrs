"""Bounded development interoperability for the current M3 writer foundation."""
import json
from pathlib import Path
import struct
import uuid

import oracle
import fixtures

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
        data = path.read_bytes()  # Explicit 197-byte development fixture only.
        if len(data) != 197 or struct.unpack_from('<6q',data,50) != (126,163,144,179,126,0):
            raise ValueError('empty writer size/major offsets differ from independent layout')
        native = run([executable,'dump',path],capture=True).stdout
        if native != expected(0,RUST_PRODUCER,RUST_PRODUCER):
            raise ValueError('native metadata/record stream differs from independent empty expectation')
        rust = run(['cargo','run','--locked','--quiet',*feature,'--example','dump','--',path],capture=True).stdout
        if rust != expected(0,RUST_PRODUCER,'<creation-system>'):
            raise ValueError('Rust metadata/record stream differs from independent empty expectation')
        cases.append(dict(direction='Rust writes → native reads',features=feature,artifact=str(path.relative_to(ROOT)),bytes=len(data),sha256=oracle.file_digest(path),native_complete_stream_matches=True,rust_complete_stream_matches=True))
    recipe = area/'empty.recipe.tsv'
    recipe.write_text('# No UELs or symbols; upstream initializes empty set-text ID 0.\n')
    for compression in [0,1]:
        path = area/f'native-empty-{compression}.gdx'
        run([executable,'recipe',path,recipe,str(compression)])
        native = run([executable,'dump',path],capture=True).stdout
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
    return dict(scope='task 018 empty-v7 foundation only',cases=cases,root=str(area.relative_to(ROOT)),native_executable_sha256=oracle.file_digest(executable),pinned_sources=json.loads((ROOT/'tools/oracle/sources.json').read_text()))
