import copy,sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).parents[1]))
import m5_scale as m

def worker(phases):
 return dict(result=dict(root_limit=m.BUDGET,root_peak=1000,os_cap_verified=True,index_quota=m.QUOTA,handle_limit=2,cache_limit=1024,phases=phases),os_cap=dict(soft_bytes=m.CAP,hard_bytes=m.CAP),peak_rss_bytes=10000)
def query(seconds,work):
 return dict(query='leading',seconds=seconds,count=10,digest='0011',returned_rows=10,decoded_rows=work,bytes_read=work*1000,decoded_bytes=work*1000,decoded_buffers=work,index_pages_read=1,validation_bytes_hashed=999,validation_pages=100)
def trial():
 cold=query(2.,1000);warm=query(.1,10)
 opened=dict(complete=True,published=False,seconds=3.,digest_seconds=1.,replay_seconds=2.,directory_bytes=m.CAP+1,pages_read=100)
 return dict(cold_serial=worker([cold]),build=worker([dict(opened,published=True,seconds=5.)]),session=worker([opened,dict(warm,seconds=.2),warm,warm,warm]),serial_matrix=worker([cold]),index=dict(bytes=4*m.CAP+1))
def cases():
 return [dict(kind=kind,count=n,logical_bytes=4*m.CAP+1,source=dict(bytes=4*m.CAP+1),trials=[copy.deepcopy(trial())for _ in range(3)])for kind in ['records','metadata','directory']for n in [20_000_000,36_000_000]]
class M5Guards(unittest.TestCase):
 def test_os_root_and_resource_limits_are_mandatory(self):
  good=worker([{}]);m.validate_worker(good)
  for group,key,value in [('result','root_limit',m.BUDGET+1),('result','root_peak',m.BUDGET+1),('result','os_cap_verified',False),('result','handle_limit',3),('result','index_quota',m.QUOTA+1),('os_cap','soft_bytes',m.CAP*2),('os_cap','hard_bytes',m.CAP*2)]:
   bad=copy.deepcopy(good);bad[group][key]=value
   with self.assertRaises(ValueError):m.validate_worker(bad)
 def test_all_independent_sizes_growing_inputs_and_trials_are_required(self):
  good=cases();m.qualify(good)
  for mutate in [lambda c:c.pop(),lambda c:c[-1]['trials'].pop(),lambda c:c[-1]['trials'][0]['build']['result']['phases'][0].update(directory_bytes=m.CAP),lambda c:c[-1]['trials'][0]['index'].update(bytes=4*m.CAP),lambda c:c[1]['source'].update(bytes=4*m.CAP),lambda c:c[3].update(logical_bytes=4*m.CAP),lambda c:c[3].update(count=30_000_000),lambda c:c[-1]['trials'][0]['session'].update(peak_rss_bytes=65*1024**2)]:
   bad=copy.deepcopy(good);mutate(bad)
   with self.assertRaises(ValueError):m.qualify(bad)
 def test_latency_and_complete_index_physical_work_cannot_be_hidden(self):
  good=cases()[1];result=m.performance(good);self.assertGreater(result['build_and_reopen_break_even_queries'],0)
  for field,value in [('seconds',2.),('decoded_rows',300),('bytes_read',300000),('decoded_buffers',300),('index_pages_read',300)]:
   bad=copy.deepcopy(good)
   for t in bad['trials']:
    for r in t['session']['result']['phases'][1:]:r[field]=value
   with self.assertRaises(ValueError):m.performance(bad)
 def test_exact_ordered_results_and_validation_snapshot_are_required(self):
  t=trial();opened=t['session']['result']['phases'][0];opened['source_bytes_hashed']=999
  m.compare_queries(t['serial_matrix'],t['session'],t['cold_serial'],999)
  for field,value in [('digest','different'),('count',9),('returned_rows',9),('validation_pages',101),('validation_bytes_hashed',998)]:
   bad=copy.deepcopy(t);bad['session']['result']['phases'][2][field]=value
   with self.assertRaises(ValueError):m.compare_queries(bad['serial_matrix'],bad['session'],bad['cold_serial'],999)
