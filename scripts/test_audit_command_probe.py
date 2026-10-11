#!/usr/bin/env python3
"""Probe assertions must reject wrong exits, channels and fabricated successes."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import unittest

spec = importlib.util.spec_from_file_location('probe', Path(__file__).with_name('check-audit-commands.py'))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class ProbeTests(unittest.TestCase):
    def case(self, code=0, fields=None):
        return {'id':'synthetic-assertion', 'group':'preservation', 'expected':{
            'exit_code':code, 'kind':'diagnostic' if code==2 else 'audit-gate',
            'fields':fields or {}}}

    def output(self, code=0, **fields):
        data={'format_version':1,'kind':'diagnostic' if code==2 else 'audit-gate',**fields}
        payload=json.dumps(data).encode()
        return subprocess.CompletedProcess([],code,b'' if code==2 else payload,payload if code==2 else b'')

    def test_current_corpus_shape_and_all_raw_input_digests(self):
        value=probe.load_corpus()
        self.assertEqual(len(value['cases']),87)
        self.assertEqual(len(value['fixtures']),30)

    def test_duplicate_fixture_bytes_are_not_normalized(self):
        value=probe.load_corpus()
        data=probe.fixture_bytes(value['fixtures']['duplicate-overall-last-high'])
        self.assertIn(b'"overall":0,"overall":100',data)

    def test_unchanged_success(self):
        self.assertEqual(probe.mismatches(self.case(fields={'passed':True}),self.output(passed=True)),[])

    def test_policy_failure_is_distinct_from_invalid_input(self):
        self.assertEqual(probe.mismatches(self.case(1,{'passed':False}),self.output(1,passed=False)),[])
        self.assertIn('exit-code',probe.mismatches(self.case(2),self.output(1,passed=False)))

    def test_diagnostic_requires_empty_stdout(self):
        output=self.output(2); output.stdout=b'{}'
        self.assertIn('unexpected-output-channel',probe.mismatches(self.case(2),output))

    def test_wrong_expected_result_fails(self):
        self.assertIn('field:passed',probe.mismatches(self.case(fields={'passed':False}),self.output(passed=True)))

    def test_missing_null_field_is_not_present_null(self):
        self.assertIn('field:missing',probe.mismatches(self.case(fields={'missing':None}),self.output()))
        self.assertEqual(probe.mismatches(self.case(fields={'missing':None}),self.output(missing=None)),[])

    def test_boolean_does_not_pass_as_numeric(self):
        self.assertIn('field:passed',probe.mismatches(self.case(fields={'passed':True}),self.output(passed=1)))
        self.assertFalse(probe._equal(True,1))

    def test_integer_and_fractional_numeric_spelling(self):
        self.assertTrue(probe._equal(0.0,0))
        self.assertTrue(probe._equal(80.25,80.25))
        self.assertFalse(probe._equal(80.25,80))

    def test_plain_text_and_non_objects_cannot_pass(self):
        for payload in (b'OK',b'[]',b'null',b'1'):
            output=subprocess.CompletedProcess([],0,payload,b'')
            self.assertTrue(probe.mismatches(self.case(),output))

    def test_changed_fixture_digest_rejected(self):
        record=copy.deepcopy(probe.load_corpus()['fixtures']['good']); record['input']+=' '
        with self.assertRaises(ValueError): probe.fixture_bytes(record)

    def test_process_environment_excludes_provider_and_shell_state(self):
        env=probe.isolated_environment(Path('/synthetic'))
        self.assertNotIn('PATH',env)
        self.assertNotIn('TYPESAFE_API_KEY',env)
        self.assertNotIn('GITHUB_TOKEN',env)


if __name__=='__main__':
    unittest.main()
