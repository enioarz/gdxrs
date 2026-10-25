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


class ReaderSuites(unittest.TestCase):
    def test_rustdoc_groups_require_real_borrowing_test_and_no_skips(self):
        summary = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
        borrowing = "test src/scan.rs - scan::BatchView (line 73) - compile fail ... ok\n"
        self.assertEqual(dev.test_suite_counts(summary*3+borrowing),[1,1,1])
        self.assertEqual(dev.test_suite_counts(summary*4+borrowing),[1,1,1,1])
        for bad in [summary*4, summary*2+borrowing, summary*3+summary.replace('0 ignored','1 ignored')+borrowing, summary*3+summary.replace('1 passed','0 passed')+borrowing]:
            with self.assertRaises(dev.CheckFailure): dev.test_suite_counts(bad)


class PortableReaderDispatch(unittest.TestCase):
    def test_reader_command_never_imports_unix_only_development_modules(self):
        import builtins
        original_import = builtins.__import__
        blocked = {'resource', 'fixtures', 'scale', 'm0', 'benchmark', 'oracle_limits'}
        def guard(name, *args, **kwargs):
            if name.split('.')[0] in blocked:
                raise AssertionError('portable reader imported ' + name)
            return original_import(name, *args, **kwargs)
        with patch.object(sys, 'argv', ['dev.py', 'reader-check']), \
             patch.object(dev, 'report_check', side_effect=lambda name, operation: operation()), \
             patch.object(dev, 'reader_check', return_value=0) as reader, \
             patch.object(builtins, '__import__', side_effect=guard):
            self.assertEqual(dev.main(), 0)
        reader.assert_called_once()

    def test_scale_command_reports_unsupported_windows_before_resource_import(self):
        with patch.object(sys, 'argv', ['dev.py', 'm2-scale-check']), \
             patch.object(dev.sys, 'platform', 'win32'), \
             patch.object(dev, 'report_check', side_effect=lambda name, operation: operation()):
            with self.assertRaisesRegex(dev.CheckFailure, 'requires Linux'):
                dev.main()


if __name__ == "__main__":
    unittest.main()
