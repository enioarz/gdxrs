"""Runner behavior tests, not GDX library tests."""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("dev", Path(__file__).parents[1] / "dev.py")
dev = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dev)


class Reports(unittest.TestCase):
    def test_failed_child_cannot_reuse_success(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(dev, "WORK", Path(directory)), patch.object(dev, "ROOT", Path(directory)):
                with patch.object(dev, "revision", return_value="test-revision"):
                    self.assertEqual(dev.report_check("probe", lambda: {"n": 1}), 0)
                    def fail():
                        return dev.run([sys.executable, "-c", "raise SystemExit(7)"])
                    self.assertEqual(dev.report_check("probe", fail), 7)
            reports = [json.loads(p.read_text()) for p in Path(directory).glob("reports/*.json")]
            self.assertEqual(len(reports), 2)
            self.assertEqual({r["status"] for r in reports}, {"passed", "failed"})
            self.assertEqual(len({r["run_id"] for r in reports}), 2)

    def test_unknown_command_fails(self):
        result = subprocess.run(
            [sys.executable, str(Path(dev.__file__)), "not-a-command"],
            capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("invalid choice", result.stderr)

    def test_missing_tool_is_failure(self):
        with self.assertRaises(FileNotFoundError):
            dev.run(["/no/such/gdxrs-tool"])

    def test_unexpected_error_records_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(dev,"WORK",Path(directory)),patch.object(dev,"ROOT",Path(directory)),patch.object(dev,"revision",return_value="test"):
                def fail(): raise KeyError('missing report field')
                self.assertEqual(dev.report_check('unexpected',fail),1)
            reports=list(Path(directory).glob('reports/*.json'))
            self.assertEqual(len(reports),1)
            self.assertEqual(json.loads(reports[0].read_text())['status'],'failed')

class DependencyAudit(unittest.TestCase):
    def model(self, extra_features=None):
        return {
            'packages': [{'id': n, 'name': n, 'version': v} for n,v in [('gdxrs','0.0.0'),('miniz_oxide','0.9.1'),('adler2','2.0.1')]],
            'resolve': {'nodes':[{'id':n, 'features':extra_features if n=='miniz_oxide' and extra_features else []} for n in ['gdxrs','miniz_oxide','adler2']]}
        }
    def test_audited_graph_rejects_changed_versions_and_features(self):
        dev.audit_dependencies(self.model())
        changed = self.model()
        changed['packages'][1]['version'] = '0.9.2'
        with self.assertRaises(dev.CheckFailure): dev.audit_dependencies(changed)
        with self.assertRaises(dev.CheckFailure): dev.audit_dependencies(self.model(['with-alloc']))
        with self.assertRaises(dev.CheckFailure): dev.audit_dependencies(self.model(), compression=False)


if __name__ == "__main__":
    unittest.main()
