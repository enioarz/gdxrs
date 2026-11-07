import copy,sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).parents[1]))
import parallel_check as p

def trial(workers=2,query='all'):
    active=0 if query=='zero'else workers
    count=0 if query=='zero'else 4
    return dict(exit_code=0,os_cap=dict(soft_bytes=p.m5.CAP,hard_bytes=p.m5.CAP),peak_rss_bytes=10000,sampled_peak_fds=5,
        result=dict(query=query,count=count,digest='abc',returned_rows=count,decoded_rows=count,produced_rows=count,
            root_limit=p.m5.BUDGET,root_peak=1000,os_cap_verified=True,vm_peak_bytes=100000,
            requested=workers,admitted=active,joined=active,maximum_buffered_batches=active,peak_ready_batches=active,
            stack_reserved=active*(2*1024**2+64*1024),control_reserved=active*256*1024,
            complete=query!='cancel',cancelled=query=='cancel',tasks_dispatched=active,tasks_completed=active,query_bytes_read=0))
class ParallelGuards(unittest.TestCase):
    def test_exact_ordered_results_are_required(self):
        t=trial();p.validate(t,2,'all',dict(count=4,digest='abc'))
        for field,value in [('digest','wrong'),('count',3),('returned_rows',3),('decoded_rows',3),('query','head')]:
            bad=trial();bad['result'][field]=value
            with self.assertRaises(ValueError):p.validate(bad,2,'all',dict(count=4,digest='abc'))
    def test_root_process_vm_and_descriptors_are_checked(self):
        mutations=[lambda t:t['result'].update(root_limit=p.m5.BUDGET+1),lambda t:t['result'].update(root_peak=p.m5.BUDGET+1),
            lambda t:t['result'].update(os_cap_verified=False),lambda t:t['result'].update(vm_peak_bytes=p.m5.CAP+1),
            lambda t:t.update(peak_rss_bytes=p.m5.CAP+1),lambda t:t.update(sampled_peak_fds=6),
            lambda t:t['os_cap'].update(soft_bytes=p.m5.CAP+1),lambda t:t['os_cap'].update(hard_bytes=p.m5.CAP+1)]
        for change in mutations:
            bad=trial();change(bad)
            with self.assertRaises(ValueError):p.validate(bad,2,'all',dict(count=4,digest='abc'))
    def test_workers_stacks_joins_and_queues_cannot_be_omitted(self):
        for field,value in [('admitted',1),('joined',1),('maximum_buffered_batches',3),('peak_ready_batches',3),('stack_reserved',0),('control_reserved',0)]:
            bad=trial();bad['result'][field]=value
            with self.assertRaises(ValueError):p.validate(bad,2,'all',dict(count=4,digest='abc'))
    def test_cancellation_is_not_exhaustion_and_zero_is_lazy(self):
        t=trial(query='cancel');p.validate(t,2,'cancel',dict(count=4,digest='abc'))
        t['result']['complete']=True
        with self.assertRaises(ValueError):p.validate(t,2,'cancel',dict(count=4,digest='abc'))
        for field in ['decoded_rows','query_bytes_read','produced_rows','tasks_dispatched']:
            bad=trial(query='zero');bad['result'][field]=1
            with self.assertRaises(ValueError):p.validate(bad,2,'zero',dict(count=0,digest='abc'))
    def test_every_codec_worker_and_query_is_required(self):
        cases=[dict(codec=codec,trials=[trial(w,q)for w in [2,4]for q in ['all','head','zero','cancel']])for codec in ['raw','zlib']]
        p.validate_matrix(cases)
        for mutate in [lambda c:c.pop(),lambda c:c[-1].update(codec='raw'),lambda c:c[-1]['trials'].pop(),lambda c:c[-1]['trials'][-1]['result'].update(query='all')]:
            bad=copy.deepcopy(cases);mutate(bad)
            with self.assertRaises(ValueError):p.validate_matrix(bad)
