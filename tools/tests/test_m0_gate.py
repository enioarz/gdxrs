"""A good baseline cannot silently approve the author's own M0 work."""

import copy
import json
from pathlib import Path
import sys
import subprocess
import unittest
import tempfile
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).parents[1]))
import benchmark
import m0


class ReviewGate(unittest.TestCase):
    def test_measured_revision_must_contain_exact_sources(self):
        old=subprocess.check_output(['git','rev-list','--max-parents=0','HEAD'],cwd=m0.ROOT,text=True).strip()
        with self.assertRaisesRegex(ValueError,'lacks benchmark source'):
            benchmark.fingerprint(old)
        prior='2941d6641b77eea03fc408f521e91b8fc38c4646'
        self.assertNotEqual(benchmark.fingerprint(prior),benchmark.fingerprint())

    def test_current_baseline_selection_and_escape(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); directory=root/'.work/bench';directory.mkdir(parents=True)
            with patch.object(benchmark,'ROOT',root):
                with self.assertRaisesRegex(ValueError,'no current baseline'):benchmark.current_baseline()
                (directory/'current.json').write_text(json.dumps({'artifact':'.work/bench/runs/fresh/baseline.json'}))
                self.assertEqual(benchmark.current_baseline(),root/'.work/bench/runs/fresh/baseline.json')
                (directory/'current.json').write_text(json.dumps({'artifact':'../outside.json'}))
                with self.assertRaisesRegex(ValueError,'outside'):benchmark.current_baseline()

    def test_failed_baseline_preserves_component_outcomes(self):
        from contextlib import ExitStack
        with ExitStack() as stack:
            for module,name in [(m0.oracle,'build'),(m0.oracle,'self_test'),(m0.fixtures,'check'),(m0.specification,'check'),(m0.contracts,'check')]:
                stack.enter_context(patch.object(module,name,return_value={'status':'passed'}))
            stack.enter_context(patch.object(m0.benchmark,'validate_baseline',side_effect=ValueError('bad baseline')))
            stack.enter_context(patch.object(m0,'validate_review',return_value={'status':'pending'}))
            result=m0.check(lambda *a,**kw:None,lambda:{'status':'passed'},Path('explicit.json'))
            self.assertEqual(result['automated_status'],'passed')
            self.assertEqual(result['baseline']['status'],'failed')
            self.assertEqual(result['independent_review']['status'],'pending')
            self.assertFalse(result['m1_eligible'])
            self.assertTrue(result['errors'])

    def test_pending_review_stays_pending(self):
        review=json.loads((m0.ROOT/'docs/reviews/m0.json').read_text())
        review.update(status='pending',reviewer=None)
        result=m0.validate_review(review,current_content='candidate')
        self.assertEqual(result['status'],'pending')

    def test_author_self_approval_and_stale_review_fail(self):
        review=dict(schema=1,status='accepted',implementation_author='author',reviewer='author')
        with self.assertRaisesRegex(ValueError,'must differ'):m0.validate_review(review,'candidate')
        review.update(reviewer='reviewer',reviewer_role='independent-human',
                      checklist={key:dict(accepted=True,notes='verified') for key in m0.CHECKLIST},
                      content_sha256='old')
        with self.assertRaisesRegex(ValueError,'stale'):m0.validate_review(review,'candidate')
        review.update(content_sha256='candidate',findings=[dict(requires_action=True,disposition='accepted-with-rationale')])
        with self.assertRaisesRegex(ValueError,'unresolved required findings'):m0.validate_review(review,'candidate')

    def test_corrupt_scan_result_cannot_be_a_baseline(self):
        workload=dict(rows=1,dimension=0,fields=1)
        result=dict(schema=1,rows=1,digest='incorrect',scan_seconds=1,peak_rss_bytes=1)
        with self.assertRaisesRegex(ValueError,'digest/count mismatch'):benchmark.validate_scan(result,workload)

    def test_old_revision_cannot_claim_current_reviewed_content(self):
        old=subprocess.check_output(['git','rev-list','--max-parents=0','HEAD'],cwd=m0.ROOT,text=True).strip()
        identity=m0.content_fingerprint()
        review=dict(schema=1,status='accepted',implementation_author='test-author',reviewer='test-independent',
                    reviewer_role='independent-human',content_sha256=identity,reviewed_revision=old,
                    checklist={key:dict(accepted=True,notes='unit test only') for key in m0.CHECKLIST},findings=[])
        with self.assertRaisesRegex(ValueError,'does not contain'):m0.validate_review(review,identity)


if __name__=='__main__':unittest.main()
