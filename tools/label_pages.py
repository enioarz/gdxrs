"""Bounded development-only stored/zlib string-crossing probes; no runtime writer."""
import hashlib
from pathlib import Path
import struct
import uuid
import zlib

import fixtures
import oracle

ROOT = Path(__file__).resolve().parents[1]


def check(run):
    # Same declared runtime probe as tests/cases/labels.rs, independently checked
    # against the pinned native implementation and authored label/ID sequence.
    area = ROOT / '.work' / 'label-pages' / uuid.uuid4().hex
    area.mkdir(parents=True)
    data = bytearray((ROOT/'tests/fixtures/v7/semantics-1.gdx').read_bytes())
    offsets = struct.unpack_from('<6q', data, 118)
    first, end = offsets[1], offsets[3]
    decoded = bytearray(b'\x05_UEL_' + struct.pack('<i',8192))
    for number in range(1,8193):
        label = f'k{number:05}'.encode()
        decoded += bytes([len(label)]) + label
    decoded += b'\x05_UEL_'
    cases = []
    for kind in (0,1):
        encoded = bytearray()
        for start in range(0,len(decoded),32768):
            block = decoded[start:start+32768]
            if kind:
                # Verify the same declared stored-DEFLATE zlib envelope that the
                # Rust runtime test builds without an encoder dependency.
                length=len(block)
                payload=b'\x78\x01\x01'+struct.pack('<HH',length,length^65535)+block+struct.pack('>I',zlib.adler32(block))
                if zlib.decompress(payload) != block: raise ValueError('zlib probe payload mismatch')
            else: payload=block
            encoded += bytes([kind]) + struct.pack('>H',len(payload)) + payload
        candidate=bytearray(data)
        delta = len(encoded)-(end-first)
        for n in (3,5):
            if offsets[n]: struct.pack_into('<q',candidate,118+n*8,offsets[n]+delta)
        candidate[first:end] = encoded
        path = area/f'crossing-{kind}.gdx'
        with path.open('xb') as stream: stream.write(candidate)
        native = run([oracle.executable(),'dump',path],capture=True).stdout
        labels = [line for line in native.splitlines() if line.startswith('U\t')]
        expected = [f'U\t{n}\t'+f'k{n:05}'.encode().hex() for n in range(1,8193)]
        if labels != expected: raise ValueError('native labels differ from declared probe sequence')
        rust = run(['cargo','run','--locked','--quiet','--example','dump','--',path],capture=True).stdout
        if fixtures.normalize_dump(native) != rust: raise ValueError('crossing probe Rust/native full output differs')
        cases.append(dict(kind='zlib' if kind else 'stored',artifact=str(path.relative_to(ROOT)),input_sha256=hashlib.sha256(candidate).hexdigest(),
                    metadata_labels=8192,decoded_metadata_bytes=len(decoded),blocks=2,
                    crossing_label_id=4680,full_native_rust_dump_match=True,
                    public_records=sum(line.startswith('R\t') for line in rust.splitlines())))
    return dict(cases=cases,native_executable_sha256=oracle.file_digest(oracle.executable()))
