"""Platform limit choice, inherited ceilings and visible setup failures."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parents[1]))
import fixtures
import oracle_limits
import resource


class OracleLimits(unittest.TestCase):
    def test_darwin_uses_data_and_finite_child_hard_limits(self):
        with patch.object(resource, 'getrlimit', return_value=(resource.RLIM_INFINITY,resource.RLIM_INFINITY)), patch.object(resource,'setrlimit') as set_limit:
            oracle_limits.apply_limits('darwin')
        calls=set_limit.call_args_list
        self.assertEqual(calls[0].args,(resource.RLIMIT_DATA,(256*1024*1024,256*1024*1024)))
        self.assertNotIn(resource.RLIMIT_AS,[c.args[0] for c in calls])
        self.assertEqual(len(calls),4)

    def test_darwin_never_reapplies_rejected_infinite_hard_limit(self):
        def darwin_set(kind, limits):
            if limits[1] == resource.RLIM_INFINITY:
                raise ValueError('current limit exceeds maximum limit')
            self.assertLessEqual(limits[0], limits[1])
        with patch.object(resource,'getrlimit',return_value=(resource.RLIM_INFINITY,resource.RLIM_INFINITY)), patch.object(resource,'setrlimit',side_effect=darwin_set):
            oracle_limits.apply_limits('darwin')

    def test_darwin_preserves_tighter_inherited_limits(self):
        with patch.object(resource,'getrlimit',return_value=(1,3)), patch.object(resource,'setrlimit') as set_limit:
            oracle_limits.apply_limits('darwin')
        self.assertEqual(set_limit.call_args_list[0].args,(resource.RLIMIT_DATA,(1,3)))

    def test_linux_address_space_and_lower_inherited_limit(self):
        with patch.object(resource,'getrlimit',return_value=(1,3)), patch.object(resource,'setrlimit') as set_limit:
            oracle_limits.apply_limits('linux')
        self.assertEqual(set_limit.call_args_list[0].args,(resource.RLIMIT_AS,(1,3)))

    def test_setup_errors_name_limit_and_do_not_exec(self):
        with patch.object(resource,'getrlimit',side_effect=ValueError('unsupported')), patch.object(oracle_limits.os,'execv') as execute:
            with self.assertRaisesRegex(RuntimeError,'memory limit setup failed: unsupported'):
                oracle_limits.apply_limits('darwin')
        execute.assert_not_called()

    def test_launcher_failures_are_not_negative_fixture_success(self):
        with self.assertRaisesRegex(RuntimeError,'native oracle isolation failed'):
            fixtures.observe_invalid(Path('/no/such/native-oracle'),Path('/unused.gdx'))


if __name__=='__main__':unittest.main()
