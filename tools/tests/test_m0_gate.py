"""A good baseline cannot silently approve the author's own M0 work."""

import copy
import json
from pathlib import Path
import sys
import subprocess
import unittest

sys.path.insert(0,str(Path(__file__).parents[1]))
import benchmark
import m0


class ReviewGate(unittest.TestCase):
    def test_user_confirmed_release_is_scoped_to_m0(self):
        review=dict(schema=1,status='released-user-confirmed',release_confirmation=dict(source='explicit-user',date='2026-10-24',statement='M0 reviewed and released'))
        self.assertEqual(m0.validate_review(review,'different-M1-content')['status'],'released-user-confirmed')
        review['release_confirmation']['source']='author-assumption'
        with self.assertRaisesRegex(ValueError,'explicit recorded user'):m0.validate_review(review,'new-M1-content')
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
