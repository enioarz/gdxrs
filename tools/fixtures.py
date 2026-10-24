"""Generate/check a bounded, independently expected v7 fixture corpus."""

import difflib
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

import fixture_cases
from format_probe import Probe, Cursor, LIMIT
import oracle
from oracle_limits import isolation_policy

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / 'tests/fixtures'
REQUIRED = {'semantics-0', 'semantics-1', 'key-widths', 'blocks', 'operations',
            'optional-domains', 'key-widths-be', 'semantics-0-be', 'optional-domains-absent', 'case-distinct-labels'}
NEGATIVE_IDS = {'version-5-probe','version-6-probe','future-version-probe','bad-offset',
                'bad-dimension','bad-key-tag','bad-value-tag','bad-count',
                'truncated-header','truncated-section','damaged-zlib-checksum','truncated-block'}


def digest(path):
    return oracle.file_digest(path)


def normalize_dump(text):
    lines = text.splitlines()
    if len(lines) < 2 or lines[0] != 'GDXRS-ORACLE\t1':
        raise ValueError('missing semantic dump header')
    fields = lines[1].split('\t')
    if len(fields) != 7 or fields[0] != 'F' or not bytes.fromhex(fields[6]).startswith(b'GDX Library'):
        raise ValueError('unexpected creation/system header')
    fields[6] = '<creation-system>'
    lines[1] = '\t'.join(fields)
    return '\n'.join(lines) + '\n'


def compare(expected, actual, label):
    if actual != expected:
        diff = '\n'.join(list(difflib.unified_diff(expected.splitlines(), actual.splitlines(), fromfile='expected', tofile='actual'))[:50])
        raise ValueError(f'{label}: expectation mismatch\n{diff}')


def generator_identity():
    paths = ['tools/fixture_cases.py', 'tools/fixtures.py', 'tools/format_probe.py',
             'tools/oracle/driver.cpp', 'tools/oracle/fixture_generator.hpp', 'tools/oracle_limits.py']
    return {name: digest(ROOT/name) for name in paths}


def save_json(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def generate(run, destination):
    destination = Path(destination).resolve()
    if destination.exists():
        raise ValueError('generation output must be a new directory; tracked expectations are never overwritten')
    destination.mkdir(parents=True)
    valid = destination/'v7'; valid.mkdir()
    invalid = destination/'invalid'; invalid.mkdir()
    exe = oracle.executable()
    items = []
    models = fixture_cases.cases()
    for case in models:
        recipe = valid/(case.name+'.recipe.tsv')
        expectation = valid/(case.name+'.expected.tsv')
        # Write independent expected results BEFORE generating or reading GDX bytes.
        recipe.write_text('\n'.join(case.recipe)+'\n')
        expectation.write_text(case.expectation())
        file = valid/(case.name+'.gdx')
        events = run([exe, 'recipe', file, recipe, str(case.compression)], capture=True).stdout
        compare('\n'.join(case.events)+ ('\n' if case.events else ''), events, case.name+' operations')
        compare(expectation.read_text(), normalize_dump(run([exe,'dump',file], capture=True).stdout), case.name)
        items.append(entry(file, expectation, recipe, destination, events))
    original = (valid/'semantics-0.gdx').read_bytes()
    p = Probe(original); start = p.offsets[1]
    cursor = Cursor(original[start:p.offsets[3]], p.order)
    cursor.marker(b'_UEL_'); count = cursor.count()
    rename_at = None
    for n in range(1,count+1):
        at = start+cursor.at+1; label = cursor.string()
        if n == 3:
            if label != b'C': raise ValueError('unexpected source label for case transform')
            rename_at = at
    file = valid/'case-distinct-labels.gdx'
    expectation = valid/'case-distinct-labels.expected.tsv'
    expectation.write_text((valid/'semantics-0.expected.tsv').read_text().replace('U\t3\t43\n', 'U\t3\t41\n'))
    transformed = bytearray(original); transformed[rename_at] = ord('A'); file.write_bytes(transformed)
    compare(expectation.read_text(), normalize_dump(run([exe,'dump',file],capture=True).stdout), 'case-distinct-labels')
    items.append(entry(file,expectation,None,destination,'',transform=dict(source='semantics-0',kind='rename-one-label-byte',offset=rename_at,from_hex='43',to_hex='41')))
    for source_id in ['key-widths', 'semantics-0']:
        file = valid/(source_id+'-be.gdx')
        original = valid/(source_id+'.gdx')
        file.write_bytes(Probe(original.read_bytes()).opposite_endian())
        expectation = valid/(source_id+'-be.expected.tsv')
        expectation.write_bytes((valid/(source_id+'.expected.tsv')).read_bytes())
        compare(expectation.read_text(), normalize_dump(run([exe,'dump',file], capture=True).stdout), source_id+'-be')
        items.append(entry(file, expectation, None, destination, '', transform=dict(source=source_id, kind='reverse-annotated-numeric-fields')))
    original = (valid/'optional-domains.gdx').read_bytes()
    p = Probe(original)
    cropped = bytearray(original[:p.offsets[5]])
    offset = p.index_offset+4+5*8
    cropped[offset:offset+8] = b'\0'*8
    file = valid/'optional-domains-absent.gdx'; file.write_bytes(cropped)
    expectation = valid/'optional-domains-absent.expected.tsv'
    expectation.write_bytes((valid/'optional-domains.expected.tsv').read_bytes())
    compare(expectation.read_text(), normalize_dump(run([exe,'dump',file], capture=True).stdout), 'optional-domains-absent')
    items.append(entry(file, expectation, None, destination, '', transform=dict(source='optional-domains', kind='omit-empty-domain-section')))
    negatives = negative_cases(destination, exe)
    manifest = dict(schema=1, upstream=json.loads((ROOT/'tools/oracle/sources.json').read_text()),
                    generator=dict(base_revision=run(['git','rev-parse','HEAD'],capture=True).stdout.strip(), files=generator_identity()),
                    fixtures=items, negatives=negatives)
    save_json(destination/'manifest.json', manifest)
    return dict(valid_fixtures=len(items), negative_fixtures=len(negatives), output=str(destination), oracle_isolation=isolation_policy())


def entry(file, expectation, recipe, root, events, transform=None):
    probe = Probe(file.read_bytes())
    return dict(id=file.stem, path=str(file.relative_to(root)), sha256=digest(file),
                expectation=str(expectation.relative_to(root)), expectation_sha256=digest(expectation),
                recipe=str(recipe.relative_to(root)) if recipe else None,
                recipe_sha256=digest(recipe) if recipe else None,
                transform=transform, expected_events=events, observations=probe.summary())


def negative_cases(root, exe):
    source = root/'v7/optional-domains.gdx'
    original = source.read_bytes(); p = Probe(original)
    first = p.records[0]
    mutations = [
        ('version-5-probe', p.version_offset, (5).to_bytes(4,'little'), 'UnsupportedVersion'),
        ('version-6-probe', p.version_offset, (6).to_bytes(4,'little'), 'UnsupportedVersion'),
        ('future-version-probe', p.version_offset, (8).to_bytes(4,'little'), 'UnsupportedVersion'),
        ('bad-offset', p.index_offset+4, (len(original)+1).to_bytes(8,'little'), 'InvalidOffset'),
        ('bad-dimension', first['dimension_offset'], b'\x15', 'InvalidDimension'),
        ('bad-key-tag', first['record_offset'], b'\0', 'InvalidKeyTag'),
        ('bad-value-tag', first['record_offset']+5, b'\x0b', 'InvalidValueTag'),
        ('bad-count', p.offsets[0]+7, b'\xff'*4, 'InvalidCount'),
    ]
    records = []
    for name, at, payload, error in mutations:
        data = bytearray(original); data[at:at+len(payload)] = payload
        records.append((name, bytes(data), error, dict(source='optional-domains', offset=at, replace_hex=payload.hex())))
    for name, length in [('truncated-header', 12), ('truncated-section', len(original)-1)]:
        records.append((name, original[:length], 'UnexpectedEof', dict(source='optional-domains', truncate_to=length)))
    block_source = root/'v7/blocks.gdx'; data = bytearray(block_source.read_bytes()); bp = Probe(bytes(data))
    block = next(b for b in bp.blocks if b['tag'] == 1)
    at = block['physical']+3+block['encoded']-1
    data[at] ^= 1
    records.append(('damaged-zlib-checksum', bytes(data), 'InvalidCompression', dict(source='blocks', xor_offset=at, xor=1)))
    records.append(('truncated-block', block_source.read_bytes()[:block['physical']+4], 'UnexpectedEof', dict(source='blocks', truncate_to=block['physical']+4)))
    entries = []
    for name, data, error, mutation in records:
        file = root/'invalid'/(name+'.gdx'); file.write_bytes(data)
        # The C++ reference is permissive on malformed data. Run in a subprocess
        # with CPU/address-space limits; report acceptance/crash/timeout honestly.
        result = observe_invalid(exe, file)
        entries.append(dict(id=name,path=str(file.relative_to(root)),sha256=digest(file),
                            intended_rust_error=error, mutation=mutation,
                            genuine_old_format=False if 'version' in name else None,
                            upstream_observation=result))
    return entries


def observe_invalid(exe, path):
    if sys.platform not in ('linux', 'darwin'):
        raise RuntimeError('native oracle isolation is only supported on Linux/macOS')
    try:
        with tempfile.TemporaryFile() as output:
            result = subprocess.run([sys.executable,str(ROOT/'tools/oracle_limits.py'),str(exe),'dump',str(path)],
                                    stdout=output,stderr=output,timeout=5)
            if result.returncode == 125:
                output.seek(0)
                detail = output.read(4096).decode('utf-8', errors='replace')
                raise RuntimeError('native oracle isolation failed: '+detail)
            return dict(outcome='accepted' if result.returncode == 0 else 'failed', exit_code=result.returncode)
    except subprocess.TimeoutExpired:
        return dict(outcome='timeout', exit_code=None)


def checked_path(root, relative):
    path = (root/relative).resolve()
    if not path.is_relative_to(root.resolve()) or not path.is_file():
        raise ValueError(f'missing/outside fixture path: {relative}')
    return path


def check(run, root=FIXTURES):
    if sys.platform == 'darwin':
        print('Oracle isolation: macOS memory limit unavailable; CPU/output/core limits and timeout remain.', flush=True)
    root = Path(root)
    manifest = json.loads((root/'manifest.json').read_text())
    if manifest.get('schema') != 1:
        raise ValueError('unsupported fixture manifest')
    items = manifest['fixtures']; ids = [item['id'] for item in items]
    if len(ids) != len(set(ids)) or set(ids) != REQUIRED:
        raise ValueError('missing/duplicate/unexpected mandatory fixture IDs')
    if manifest['upstream'] != json.loads((ROOT/'tools/oracle/sources.json').read_text()):
        raise ValueError('oracle source pins changed')
    if manifest['generator']['files'] != generator_identity():
        raise ValueError('generator changed; review/regenerate into a new directory')
    exe = oracle.executable(); probes = {}; total = 0
    for item in items:
        file = checked_path(root,item['path']); expected = checked_path(root,item['expectation'])
        for path, wanted in [(file,item['sha256']),(expected,item['expectation_sha256'])]:
            if digest(path) != wanted:
                raise ValueError(f'fixture digest mismatch: {path.name}')
        if item['recipe']:
            path = checked_path(root,item['recipe'])
            if digest(path) != item['recipe_sha256']: raise ValueError('recipe digest mismatch')
        total += file.stat().st_size
        if total > LIMIT: raise ValueError('binary fixture corpus exceeds 16 MiB')
        probe = Probe(file.read_bytes()); probes[item['id']] = probe
        if probe.summary() != item['observations']: raise ValueError('byte-observation mismatch')
        compare(expected.read_text(), normalize_dump(run([exe,'dump',file],capture=True).stdout), item['id'])
    verify_coverage(probes)
    negative_ids = [item['id'] for item in manifest['negatives']]
    if set(negative_ids) != NEGATIVE_IDS or len(negative_ids) != len(NEGATIVE_IDS):
        raise ValueError('negative fixture coverage mismatch')
    for item in manifest['negatives']:
        path = checked_path(root,item['path'])
        total += path.stat().st_size
        if total > LIMIT: raise ValueError('binary fixture corpus exceeds 16 MiB')
        if digest(path) != item['sha256']: raise ValueError('negative fixture digest mismatch')
        if observe_invalid(exe,path) != item['upstream_observation']:
            raise ValueError('changed upstream malformed-input observation: '+item['id'])
        try:
            Probe(path.read_bytes())
        except (ValueError, OverflowError):
            pass
        else: raise ValueError('development probe accepted malformed fixture: '+item['id'])
    return dict(valid_fixtures=len(items), negative_fixtures=len(negative_ids), binary_bytes=total,
                symbols=sum(len(p.symbols) for p in probes.values()),
                stored_records=sum(p.summary()['stored_records'] for p in probes.values()),
                verified_coverage='all required task-003 groups', unverified=[], oracle_isolation=isolation_policy())


def verify_coverage(probes):
    symbols = [s for p in probes.values() for s in p.symbols]
    if {s['type'] for s in symbols} != set(range(5)): raise ValueError('symbol-kind coverage')
    if not {0,1,2,20} <= {s['dimension'] for s in symbols}: raise ValueError('dimension coverage')
    if not set(range(1,10)) <= {s['user'] for s in symbols if s['type']==2}: raise ValueError('variable defaults')
    if not set(range(53,60)) <= {s['user'] for s in symbols if s['type']==3}: raise ValueError('equation defaults')
    if probes['key-widths'].summary()['widths'] != [1,2,4]: raise ValueError('key-width coverage')
    if probes['semantics-0'].summary()['tags'] != list(range(11)): raise ValueError('value-tag coverage')
    for name in ('semantics-0', 'semantics-1'):
        p = probes[name]
        equations = {s['id'] for s in p.symbols if s['type'] == 3}
        if not any(r['symbol'] in equations and any(len(values) == 5 and len(set(values)) == 5 for _, values in r['rows']) for r in p.records):
            raise ValueError('stored five-field equation coverage')
    b = probes['blocks'].summary()
    if not b['compressed_blocks'] or not b['stored_blocks'] or not b['crossing_records'] or not b['multiple_records_per_block']:
        raise ValueError('compression-boundary coverage')
    if probes['key-widths-be'].order != 'big' or probes['semantics-0-be'].order != 'big': raise ValueError('byte-order coverage')
    if probes['optional-domains-absent'].offsets[5] != 0: raise ValueError('optional-domain coverage')
    s = probes['semantics-0']
    if b'\xff' not in s.tables['_UEL_'] or max(map(len,s.tables['_UEL_'])) != 63: raise ValueError('label bytes/limits')
    if not s.relaxed or not any(x['domains'] for x in s.symbols): raise ValueError('domain metadata coverage')
    if not {b'a',b'A'} <= set(probes['case-distinct-labels'].tables['_UEL_']): raise ValueError('case-distinct stored labels')


def regeneration_check(run, root=FIXTURES):
    with tempfile.TemporaryDirectory(dir=ROOT/'.work',prefix='regenerate-') as temp:
        out = Path(temp)/'corpus'
        generate(run,out)
        original = json.loads((root/'manifest.json').read_text())
        regenerated = json.loads((out/'manifest.json').read_text())
        for before, after in zip(original['fixtures'], regenerated['fixtures'], strict=True):
            compare((root/before['expectation']).read_text(),(out/after['expectation']).read_text(),'regenerated expectation')
            a = Probe((root/before['path']).read_bytes()); b = Probe((out/after['path']).read_bytes())
            if a.summary() != b.summary(): raise ValueError('regenerated byte properties differ')
            # Audit/system identification may vary by creation time. All other
            # bytes must match on this pinned build/platform after masking it.
            left = bytearray(a.data); right = bytearray(b.data)
            for data,p in [(left,a),(right,b)]:
                at = p.version_offset+8
                size = data[at]; data[at+1:at+1+size] = b'\0'*size
            if left != right: raise ValueError('nondeterministic bytes outside creation/system string')
        return dict(regenerated=len(regenerated['fixtures']), comparisons='full semantics, byte properties, bytes except audit string', oracle_isolation=isolation_policy())
