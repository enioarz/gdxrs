"""Validate specified reader acceptance coverage; does NOT execute reader behavior."""

import json
from pathlib import Path
import re

import fixtures

ROOT = Path(__file__).resolve().parents[1]
AREAS = {'ids','values','labels','sources','batches','metadata','limits','errors','resources'}
SCENARIOS = {
    'open-no-record-scan','first-batch','zero-limit','scalar-explicit','scalar-default',
    'empty-indexed','all-kinds-and-fields','universe-and-alias','special-identities',
    'raw-label-bytes','metadata-cold-lookup','projection-framing','empty-projection',
    'unknown-version','malformed-fields','checksum-integrity','short-reads-and-terminal-error',
    'batch-too-small','budget-rejection-and-drop','borrowed-batch-reuse',
    'source-handle-mismatch','source-truncation','both-byte-orders',
    'm2-empty-selection','m2-large-scale',
}


def check(docs=None):
    docs = ROOT/'docs' if docs is None else Path(docs)
    model = json.loads((docs/'reader-acceptance.json').read_text())
    if model.get('schema') != 1 or model.get('validation_kind') != 'documentation-contract-coverage-only':
        raise ValueError('contract schema/validation-kind mismatch')
    if set(model['areas']) != AREAS: raise ValueError('required contract area missing')
    rows = model['scenarios']; ids = [row['id'] for row in rows]
    if len(ids) != len(set(ids)) or set(ids) != SCENARIOS: raise ValueError('missing/duplicate required acceptance scenario')
    choices = model['choices']
    if choices['minimum_budget_bytes'] != 1048576 or choices['default_budget_bytes'] != 67108864:
        raise ValueError('budget defaults changed without contract revision')
    if choices['batch_ownership'] != 'cursor-owned-lending' or choices['workers_in_m1'] != 0:
        raise ValueError('initial ownership/concurrency contract changed')
    if choices['codec_ceiling_measured'] is not False:
        raise ValueError('M0 must not claim unexecuted codec measurement')
    manifest = json.loads((fixtures.FIXTURES/'manifest.json').read_text())
    items = {item['id']:item for item in manifest['fixtures']}
    rule_ids = {row['id'] for row in json.loads((docs/'format-evidence.json').read_text())['rules']}
    covered = set()
    for row in rows:
        if row['status'] != 'specified-not-executed': raise ValueError('contract checker cannot certify runtime execution')
        if row['fixture'] not in items: raise ValueError('unknown fixture reference')
        if not row['rule_ids'] or not set(row['rule_ids']) <= rule_ids: raise ValueError('unknown/missing format-rule reference')
        if not row['areas'] or not set(row['areas']) <= AREAS: raise ValueError('unknown contract area')
        covered.update(row['areas'])
        if row['stage'] not in ('M1','M2') or not row['expected'] or not row['operation'] or not row['resource_assertion'] or not row['test_method']:
            raise ValueError('incomplete acceptance scenario')
        if row['semantic_lines']:
            text = (fixtures.FIXTURES/items[row['fixture']]['expectation']).read_text().splitlines()
            if not set(row['semantic_lines']) <= set(text): raise ValueError('expected semantic anchor not present in oracle-verified fixture')
    if covered != AREAS: raise ValueError('uncovered contract area')
    for name in ['reader-contract.md','resource-contract.md','decisions/0002-value-and-label-model.md']:
        text = (docs/name).read_text()
        if len(text) < 1000 or re.search(r'\b(TBD|TODO|FIXME)\b',text):
            raise ValueError('unresolved/empty initial reader contract')
    # Arithmetic example only: actual allocation/RSS accounting remains M1/M2 work.
    example_kib = 16+128+64+32+128+16+64
    if example_kib != 448 or example_kib*1024 >= choices['minimum_budget_bytes']:
        raise ValueError('inconsistent working-set example')
    return dict(validation_kind=model['validation_kind'],specified_scenarios=len(rows),
                m1_scenarios=sum(row['stage']=='M1' for row in rows),
                future_m2_scenarios=sum(row['stage']=='M2' for row in rows),
                contract_areas=len(covered),runtime_scenarios_executed=0,
                resource_measurements_executed=0,codec_ceiling_measured=False)
