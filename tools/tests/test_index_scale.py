import copy,sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).parents[1]))
import index_scale as q
class IndexContainment(unittest.TestCase):
 def test_directory_not_just_whole_index_must_exceed_ram(self):
  cases=[dict(count=n,build=dict(result=dict(directory_bytes=q.CAP+1),peak_rss_bytes=100),opened=dict(result=dict(directory_bytes=q.CAP+1),peak_rss_bytes=100),source=dict(bytes=q.CAP+1),index=dict(bytes=4*q.CAP+1)) for n in [1,4]]
  q.qualify(cases)
  with self.assertRaises(ValueError):q.qualify(cases[:1])
  for phase in ['build','opened']:
   bad=copy.deepcopy(cases);bad[-1][phase]['result']['directory_bytes']=q.CAP
   with self.assertRaises(ValueError):q.qualify(bad)
   bad=copy.deepcopy(cases);bad[-1][phase]['peak_rss_bytes']=8*1024**2+101
   with self.assertRaises(ValueError):q.qualify(bad)
 def test_counts_root_and_both_os_limits_are_mandatory(self):
  good=dict(result=dict(count=100,intervals=100,root_limit=q.BUDGET,root_peak=100,os_cap_verified=True,index_bytes=1000),os_cap=dict(hard_bytes=q.CAP,soft_bytes=q.CAP),peak_rss_bytes=100)
  q.validate(good,100)
  for group,field,value in [('result','intervals',99),('result','root_peak',q.BUDGET+1),('os_cap','soft_bytes',q.CAP*2),('os_cap','hard_bytes',q.CAP*2)]:
   bad=copy.deepcopy(good);bad[group][field]=value
   with self.assertRaises(ValueError):q.validate(bad,100)
