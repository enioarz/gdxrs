"""Aggregate M0 checks and a non-self-approving independent review gate."""

import hashlib
import json
from pathlib import Path
import subprocess

import benchmark
import contracts
import fixtures
import oracle
import specification

ROOT = Path(__file__).resolve().parents[1]
CHECKLIST = {'source-pins','fixture-independence','format-evidence','api-inventory',
             'reader-resource-contracts','benchmark-method','m1-prerequisites'}


def content_fingerprint():
    paths=subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard'],cwd=ROOT).split(b'\0')
    digest=hashlib.sha256()
    excluded={'tasks/README.md','docs/reviews/m0.md','docs/reviews/m0.json',
              'docs/benchmarks/baseline.md','docs/benchmarks/oracle-small.json'}
    for raw in sorted(set(p for p in paths if p)):
        name=raw.decode()
        if name in excluded or name.startswith('docs/evidence/'): continue
        path=ROOT/name
        if not path.is_file(): raise ValueError('missing M0 review content '+name)
        digest.update(raw+b'\0');digest.update(path.read_bytes());digest.update(b'\0')
    return digest.hexdigest()


def validate_review(review,current_content=None):
    identity=content_fingerprint() if current_content is None else current_content
    if review.get('schema')!=1: raise ValueError('unsupported independent review record')
    if review.get('status')!='accepted':
        return dict(status=review.get('status','unavailable'),current_content_sha256=identity,
                    reason='Independent reviewer has not accepted M0; author self-review is insufficient.')
    if not review.get('reviewer') or review['reviewer']==review.get('implementation_author'):
        raise ValueError('independent reviewer must differ from implementation author')
    if review.get('reviewer_role') not in ('independent-human','independent-agent'):
        raise ValueError('independent reviewer role missing')
    checklist=review.get('checklist',{})
    if set(checklist)!=CHECKLIST or any(row.get('accepted') is not True or not row.get('notes') for row in checklist.values()):
        raise ValueError('independent review checklist incomplete')
    if review.get('content_sha256')!=identity:
        raise ValueError('independent review is stale for current M0 content')
    for finding in review.get('findings',[]):
        if finding.get('requires_action') and finding.get('disposition') not in ('fixed-and-verified','accepted-with-rationale'):
            raise ValueError('independent review has unresolved required findings')
    revision=review.get('reviewed_revision')
    if not revision: raise ValueError('reviewed revision missing')
    subprocess.run(['git','merge-base','--is-ancestor',revision,'HEAD'],cwd=ROOT,check=True)
    changed=subprocess.check_output(['git','diff','--name-only',revision,'HEAD'],cwd=ROOT,text=True).splitlines()
    return dict(status='accepted',reviewer=review['reviewer'],reviewed_revision=revision,
                current_content_sha256=identity,post_review_changes=changed)


def check(run,bootstrap):
    # These run now; a retained report cannot substitute for a failing new check.
    automated={
        'bootstrap':bootstrap(),
        'oracle_build':oracle.build(run),
        'oracle_self_test':oracle.self_test(run),
        'fixtures':fixtures.check(run),
        'specification':specification.check(run),
        'reader_contracts':contracts.check(),
    }
    run(['python3','-W','error::ResourceWarning','-m','unittest','discover','-s','tools/tests','-v'],timeout=180)
    automated['tooling_tests']={'status':'passed','command':'python3 -W error::ResourceWarning -m unittest discover -s tools/tests -v'}
    baseline=benchmark.validate_baseline(ROOT/'docs/benchmarks/oracle-small.json')
    review=validate_review(json.loads((ROOT/'docs/reviews/m0.json').read_text()))
    return dict(automated_status='passed',automated=automated,baseline=baseline,independent_review=review,
                m1_eligible=review['status']=='accepted')
