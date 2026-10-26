"""Fail-closed writer acceptance checks; no large files or native execution."""
import copy
from pathlib import Path
import sys
import unittest
sys.path.insert(0,str(Path(__file__).parents[1]))
import writer_scale as w

class WriterAcceptance(unittest.TestCase):
    def test_large_records_and_metadata_are_independently_required(self):
        good=dict(bytes=4*w.CAP+1,logical_bytes=4*w.CAP+1,labels=(4*w.CAP)//64+1)
        w.qualify_large(good,'records');w.qualify_large(good,'metadata')
        for field in ['bytes','logical_bytes','labels']:
            case=good.copy();case[field]=0
            with self.assertRaises(ValueError):w.qualify_large(case,'metadata')
    def test_one_root_containment_counts_and_scratch_are_mandatory(self):
        good=dict(result=dict(count=100,labels=1024,os_cap_verified=True,root_limit=w.BUDGET,root_peak=10000,shared_root=True,scratch_peak=1000,scratch_peak_handles=6),os_cap=dict(hard_bytes=w.CAP),peak_rss_bytes=100000)
        w.validate_worker(good,100,1024,pipeline=True)
        for field,value in [('count',99),('labels',1023),('os_cap_verified',False),('root_limit',w.BUDGET+1),('root_peak',w.BUDGET+1),('shared_root',False),('scratch_peak',16*1024**3+1),('scratch_peak_handles',13)]:
            bad=copy.deepcopy(good);bad['result'][field]=value
            with self.assertRaises(ValueError):w.validate_worker(bad,100,1024,pipeline=True)
    def test_rss_growth_and_multiple_sizes_are_required(self):
        cases=[dict(count=1,worker=dict(peak_rss_bytes=10)),dict(count=4,worker=dict(peak_rss_bytes=100))]
        w.growth(cases)
        with self.assertRaises(ValueError):w.growth(cases[:1])
        cases[-1]['worker']['peak_rss_bytes']=64*1024**2+11
        with self.assertRaises(ValueError):w.growth(cases)
