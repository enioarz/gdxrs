"""A good baseline cannot silently approve the author's own M0 work."""

import copy
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0,str(Path(__file__).parents[1]))
import benchmark
import m0


class ReviewGate(unittest.TestCase):
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

    def test_corrupt_scan_result_cannot_be_a_baseline(self):
        workload=dict(rows=1,dimension=0,fields=1)
        result=dict(schema=1,rows=1,digest='incorrect',scan_seconds=1,peak_rss_bytes=1)
        with self.assertRaisesRegex(ValueError,'digest/count mismatch'):benchmark.validate_scan(result,workload)


if __name__=='__main__':unittest.main()
