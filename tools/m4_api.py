"""Source-roster and implementation-bound current M4 API audit (not M0 plan mutation)."""
import csv
import hashlib
from pathlib import Path
import re
import subprocess
ROOT=Path(__file__).resolve().parents[1]
PIN='9e2a8c8932412010cd7d2f94727d1740143bf3da'
STATES={'native-equivalent','adapter-equivalent','excluded'}
def read(path):
    with path.open(newline='',encoding='utf-8') as f:return list(csv.DictReader(f))
def validate(rows,roster):
    names=[r.get('native_function') for r in rows]
    if len(names)!=len(set(names)) or set(names)!=set(roster):raise ValueError('M4 API roster missing/duplicate/unknown native surface')
    planned=read(ROOT/"docs/api-parity.csv")+read(ROOT/"docs/api-properties.csv")
    required={r["function"] for r in planned if r["milestone"] in {"M1","M2","M3","M4"} and r["function"]!="gdxGetDLLVersion"}
    if {r["native_function"] for r in rows if r.get("required_for_m4")=="yes"}!=required:raise ValueError("required M4 roster differs from frozen milestone inventory")
    methods=set()
    for source in (ROOT/'src').rglob('*.rs'):
        methods.update(re.findall(r'\bpub\s+fn\s+(\w+)',source.read_text()))
    for row in rows:
        if row.get('disposition') not in STATES or row.get('required_for_m4') not in ['yes','no'] or row.get('surface') not in ['function','property']:raise ValueError('unclosed M4 API disposition')
        if not all(row.get(k) for k in ['rust_equivalent','semantic_contract','evidence','follow_up']):raise ValueError('missing M4 semantics/evidence/exclusion rationale')
        if row['required_for_m4']=='yes' and row['disposition']=='excluded':raise ValueError('required M4 parity gap cannot be excluded')
        if row['disposition']=='excluded' and row['follow_up']=='none':raise ValueError('excluded surface needs explicit follow-up')
        evidence=ROOT/row['evidence']
        if not evidence.is_file() or not evidence.resolve().is_relative_to((ROOT/'docs/evidence').resolve()):raise ValueError('missing/outside API evidence')
        refs=row.get('rust_methods','').split(';') if row.get('rust_methods') else []
        if any(name not in methods for name in refs):raise ValueError('API equivalent refers to missing Rust method')
        if row['disposition']!='excluded' and not refs:raise ValueError('implemented API needs concrete Rust methods')
    return {'entries':len(rows),'required':sum(r['required_for_m4']=='yes' for r in rows),'dispositions':{s:sum(r['disposition']==s for r in rows) for s in sorted(STATES)}}
def check():
    import specification
    source=ROOT/'.work/oracle/gdx'
    if subprocess.check_output(['git','-C',source,'rev-parse','HEAD'],text=True).strip()!=PIN:raise ValueError('API audit needs pinned native source')
    yaml=source/'src/gdxapi.yaml';functions,properties=specification.inventory(yaml.read_text())
    rows=read(ROOT/'docs/m4-api.csv');result=validate(rows,functions+properties)
    surface={r['native_function']:r['surface'] for r in rows}
    if any(surface[n]!='function' for n in functions) or any(surface[n]!='property' for n in properties):raise ValueError('native function/property kind mismatch')
    return dict(source_revision=PIN,native_api_sha256=hashlib.sha256(yaml.read_bytes()).hexdigest(),inventory_sha256=hashlib.sha256((ROOT/'docs/m4-api.csv').read_bytes()).hexdigest(),**result)
