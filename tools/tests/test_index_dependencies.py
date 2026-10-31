import copy,json
from pathlib import Path
import sys,unittest
sys.path.insert(0,str(Path(__file__).parents[1]))
import index_dependencies as d
class IndexDependencies(unittest.TestCase):
    def graph(self):
        pkgs=[dict(name=n,version=v,id=n) for n,v in d.PACKAGES|{('gdxrs','0.0.0')}]
        return dict(packages=pkgs,resolve=dict(nodes=[dict(id=p['id'],features=d.FEATURES.get(p['name'],[])) for p in pkgs]))
    def test_graph_versions_and_accelerated_or_unexpected_features_fail_closed(self):
        good=self.graph();d.audit(good,False)
        for change in ['version','asm','missing','new']:
            bad=copy.deepcopy(good)
            if change=='version':bad['packages'][0]['version']='99'
            elif change=='asm':next(n for n in bad['resolve']['nodes'] if n['id']=='sha2')['features']=['asm']
            elif change=='missing':bad['packages'].pop()
            else:bad['packages'].append(dict(name='unplanned',version='1',id='unplanned'))
            with self.assertRaises(ValueError):d.audit(bad,False)
