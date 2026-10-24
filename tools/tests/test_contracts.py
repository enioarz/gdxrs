"""Contract coverage validation is distinct from unimplemented runtime behavior."""

import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0,str(Path(__file__).parents[1]))
import contracts


class ContractChecks(unittest.TestCase):
    def test_missing_scenario_and_false_runtime_claim_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            docs=Path(directory)/'docs';shutil.copytree(contracts.ROOT/'docs',docs)
            path=docs/'reader-acceptance.json';original=json.loads(path.read_text())
            changed=dict(original,scenarios=original['scenarios'][:-1]);path.write_text(json.dumps(changed))
            with self.assertRaisesRegex(ValueError,'required acceptance scenario'): contracts.check(docs)
            original['scenarios'][0]['status']='executed'
            path.write_text(json.dumps(original))
            with self.assertRaisesRegex(ValueError,'cannot certify runtime execution'): contracts.check(docs)

    def test_semantic_anchor_must_exist(self):
        with tempfile.TemporaryDirectory() as directory:
            docs=Path(directory)/'docs';shutil.copytree(contracts.ROOT/'docs',docs)
            path=docs/'reader-acceptance.json';model=json.loads(path.read_text())
            model['scenarios'][0]['semantic_lines']=['not a record in the corpus']
            path.write_text(json.dumps(model))
            with self.assertRaisesRegex(ValueError,'semantic anchor'): contracts.check(docs)


if __name__=='__main__': unittest.main()
