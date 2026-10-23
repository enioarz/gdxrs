"""Source/fixture-bound checks for the M0 specification, not a Rust parser."""

import csv
import hashlib
import json
from pathlib import Path
import re
import subprocess

from format_probe import Probe
import fixtures

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT/'.work/oracle/gdx'
PIN = '9e2a8c8932412010cd7d2f94727d1740143bf3da'
RULE_IDS = {f'F{i:02}' for i in range(1,22)}


def inventory(text):
    functions = text.split('\nfunctions:\n',1)[1].split('\nproperties:\n',1)[0]
    names = re.findall(r'^  - (gdx\w+):',functions,re.M)
    if len(names) != len(set(names)) or not names:
        raise ValueError('empty/duplicate function definitions')
    properties = text.split('\nproperties:\n',1)[1].split('\nfunctionpointers:\n',1)[0]
    entries = re.split(r'^ - ',properties,flags=re.M)[1:]
    accessors = []
    for entry in entries:
        name = entry.split(':',1)[0]
        action = re.search(r'^    action\s*:\s*([rw])\s*$',entry,re.M)
        if not action or not re.fullmatch(r'gdx\w+',name): raise ValueError('unrecognized property declaration')
        accessors.append(name+('Set' if action[1]=='w' else ''))
    return names,accessors


def observations(root=fixtures.FIXTURES):
    def probe(name): return Probe((root/'v7'/f'{name}.gdx').read_bytes())
    a, k, b, o = map(probe,['semantics-0','key-widths','blocks','operations'])
    big = probe('semantics-0-be')
    raw_counts = [r['header_count'] for r in o.records]
    return {
        'F01': {'little_prefix':a.data[:17].hex(),'big_prefix':big.data[:17].hex()},
        'F02': {'version_offset':a.version_offset,'identifier':a.data[18:26].hex(),'producer':a.producer.hex()},
        'F03': {'reserved_bytes':a.data_start-a.index_offset,'index_marker':int.from_bytes(a.data[a.index_offset:a.index_offset+4],a.order),'append_equals_symbol_table':a.offsets[0]==a.offsets[4]},
        'F04': {'decoded_block_max':max(x['decoded_end']-x['decoded_start'] for x in b.blocks),'stored_blocks':b.summary()['stored_blocks'],'crossing_records':b.summary()['crossing_records']},
        'F05': {'types':sorted({s['type'] for s in a.symbols}),'dimensions':sorted({s['dimension'] for s in a.symbols}),'max_name':max(len(s['name']) for s in a.symbols),'max_description':max(len(s['description']) for s in a.symbols)},
        'F06': {'large_uel_count':len(k.tables['_UEL_']),'byte_labels':[v.hex() for v in a.tables['_UEL_']]},
        'F07': {'set_texts':[s.hex() for s in a.tables['_SETT_']]},
        'F08': {'acronyms':[[n.hex(),t.hex(),i] for n,t,i in a.acronyms]},
        'F09': {'strict':a.symbols[6]['domains'],'relaxed':a.relaxed,'absent_pointer':probe('optional-domains-absent').offsets[5]},
        'F10': {'operations_header_counts':raw_counts,'empty_scalar_stored':a.symbols[5]['count']},
        'F11': {'widths':[r['widths'] for r in k.records[:4]],'cardinalities':[r['bounds'][0][1]-r['bounds'][0][0]+1 for r in k.records[:4]]},
        'F12': {'prefix_records':[r['hex'] for r in k.records[-1]['annotations'][:4]],'prefix_keys':[r['keys'] for r in k.records[-1]['annotations'][:4]]},
        'F13': {'tags':a.summary()['tags'],'ordinary_2_5':k.records[0]['annotations'][0]['hex'][-18:]},
        'F14': {'variable_subtypes':[s['user'] for s in a.symbols if s['name'].startswith(b'v')],'equation_subtypes':[s['user'] for s in a.symbols if s['type']==3],'empty_scalar_physical':len(a.records[3]['rows'])},
        'F15': {'aliases':[[s['user'],s['position'],s['count']] for s in a.symbols if s['type']==4]},
        'F16': {'record_order':[[list(row[0]) for row in r['rows']] for r in o.records[:2]],'stored_error_count':o.symbols[3]['errors']},
        'F17': {'registered_labels':[v.hex() for v in a.tables['_UEL_']],'case_distinct_labels':[v.hex() for v in probe('case-distinct-labels').tables['_UEL_']]},
        'F18': {'appended_last_name':o.symbols[-1]['name'].hex(),'retained_symbol_names':[s['name'].hex() for s in o.symbols[:4]],'append_pointer':o.offsets[4]==o.offsets[0]},
        'F19': {'invalid_cases':len(json.loads((root/'manifest.json').read_text())['negatives']),'checksum_probe_rejected':checksum_rejected(root)},
        'F20': {'delta_records_present':any(t>2 for t in k.records[-1]['first_tags']),'cross_block_record':b.summary()['crossing_records']==1},
        'F21': {'uniform_orders':[probe('semantics-0').order,probe('semantics-0-be').order],'domain_variants':[bool(probe('optional-domains').offsets[5]),bool(probe('optional-domains-absent').offsets[5])]},
    }


def checksum_rejected(root):
    try: Probe((root/'invalid/damaged-zlib-checksum.gdx').read_bytes())
    except ValueError: return True
    return False


def read_rows(path):
    with path.open(newline='') as stream: return list(csv.DictReader(stream))


def check(run, docs=None, corpus=None):
    docs = ROOT/'docs' if docs is None else Path(docs)
    corpus = fixtures.FIXTURES if corpus is None else Path(corpus)
    head = run(['git','-C',SOURCE,'rev-parse','HEAD'],capture=True).stdout.strip()
    if head != PIN: raise ValueError('spec requires pinned source checkout')
    ledger = json.loads((docs/'format-evidence.json').read_text())
    if ledger['schema'] != 1 or ledger['source_revision'] != PIN: raise ValueError('spec ledger version/pin')
    rules = ledger['rules']; ids = [r['id'] for r in rules]
    if set(ids) != RULE_IDS or len(ids) != len(RULE_IDS): raise ValueError('missing/duplicate format rules')
    for file, wanted in ledger['source_sha256'].items():
        if not (SOURCE/file).resolve().is_relative_to(SOURCE.resolve()): raise ValueError('source path escape')
        if fixtures.digest(SOURCE/file) != wanted: raise ValueError('pinned source digest mismatch')
    fixture_ids = {x['id'] for x in json.loads((corpus/'manifest.json').read_text())['fixtures']}
    actual = observations(corpus)
    text = (docs/'format-v7.md').read_text()
    for rule in rules:
        if rule['status'] != 'verified' or not rule['fixtures'] or not rule['sources']:
            raise ValueError('unverified rule '+rule['id'])
        if not set(rule['fixtures']) <= fixture_ids: raise ValueError('unknown fixture reference')
        if f"## {rule['id']} —" not in text: raise ValueError('missing normative rule text')
        for source in rule['sources']:
            if source['path'] not in ledger['source_sha256']: raise ValueError('unbound source path')
            lines = (SOURCE/source['path']).read_text().splitlines()
            if not 1 <= source['line'] <= len(lines) or source['anchor'] not in lines[source['line']-1]:
                raise ValueError('source anchor mismatch')
        # JSON normalization deliberately makes tuple/list representation irrelevant.
        if json.loads(json.dumps(actual[rule['id']])) != rule['expected_observations']:
            raise ValueError('annotated byte/semantic observation mismatch: '+rule['id'])
    names,properties = inventory((SOURCE/'src/gdxapi.yaml').read_text())
    functions = read_rows(docs/'api-parity.csv'); accessors = read_rows(docs/'api-properties.csv')
    for expected,rows in [(names,functions),(properties,accessors)]:
        found = [row['function'] for row in rows]
        if len(found)!=len(set(found)) or set(found)!=set(expected): raise ValueError('API inventory missing/duplicate/unexpected function')
        for row in rows:
            if not all(row.values()) or row['status'] not in {'planned-native','planned-adapter','deferred-external'}:
                raise ValueError('incomplete API disposition')
            if row['milestone'] not in {f'M{i}' for i in range(1,9)}: raise ValueError('unassigned API milestone')
    return dict(rules=len(rules),verified=len(rules),unverified=0,public_functions=len(names),property_accessors=len(properties),
                observations='exact source anchors/digests and fixture byte/semantic assertions',runtime_rust_tests=0)
