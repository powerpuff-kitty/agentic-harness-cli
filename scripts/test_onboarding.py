#!/usr/bin/env python3
"""Runner regressions using a simulated CLI; real binaries are tested separately."""
from __future__ import annotations
import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import onboarding as checks

SOURCES = {
    'format': 1,
    'canonical': {'repository': 'powerpuff-kitty/agentic-harness', 'commit': 'a' * 40},
    'agents': {'repository': 'powerpuff-kitty/agentic-harness-agents', 'commit': 'b' * 40},
    'model_registry': {'repository': 'powerpuff-kitty/agentic-harness', 'commit': 'a' * 40},
}
SYNTHETIC = 'agentic' + '-harness' + '-synthetic-consumer'


def audit():
    return {'format_version': 2, 'kind': 'codebase-audit', 'overall': None,
            'scores': {'testing': None}, 'readiness': {'production': None},
            'checks': {'not_checked': ['test execution']}, 'discovered_checks': []}


class OnboardingTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.binary = self.root / 'synthetic-binary'
        self.binary.write_text('simulation only')
        fixture = self.root / checks.FIXTURE
        fixture.parent.mkdir(parents=True)
        fixture.write_bytes((checks.ROOT / checks.FIXTURE).read_bytes())
        self.mutation = None
        self.calls = []

    def simulate(self, argv, *, cwd, **options):
        self.calls.append(argv[1:])
        self.assertFalse(options['shell'])
        self.assertEqual(options['timeout'], 60)
        command = argv[1]
        output, code, error = {}, 0, ''
        if command == '--version':
            output = {'sources': copy.deepcopy(SOURCES)}
        elif command == 'init':
            target = cwd / argv[2]
            (target / '.agentic').mkdir(parents=True)
            (target / '.agentic/PRODUCT.md').write_text('# Synthetic product\n')
            (target / '.agentic/lock.json').write_text(json.dumps({
                'canonical_source': SOURCES['canonical'], 'agents_source': SOURCES['agents']}))
        elif command == 'validate':
            output = {'valid': True}
        elif command == 'audit':
            output, code = audit(), 1
            if (cwd / argv[2] / 'package.json').exists():
                output['discovered_checks'] = [{'name': 'test', 'executed': False}]
        elif command != 'upgrade':
            code = 2
            error = json.dumps({'format_version': 1, 'kind': 'diagnostic', 'code': 'invalid-input'})
        if self.mutation:
            output, code, error = self.mutation(command, cwd, argv, output, code, error)
        return subprocess.CompletedProcess(argv, code, json.dumps(output), error)

    def execute(self, sources=None):
        with patch.object(checks.subprocess, 'run', self.simulate):
            return checks.exercise(self.binary, self.root, {}, SOURCES if sources is None else sources, self.root)

    def test_fixture_and_simulated_full_loop(self):
        result = self.execute()
        self.assertTrue(result['passed'])
        self.assertEqual(len(result['checks']), 17)
        self.assertFalse(result['discovered_checks_executed'])
        self.assertFalse(list(self.root.glob('onboarding-*')))

    def test_wrong_source_identity_rejected(self):
        expected = copy.deepcopy(SOURCES)
        expected['canonical']['commit'] = 'c' * 40
        with self.assertRaisesRegex(checks.VerificationError, 'pin mismatch'):
            self.execute(expected)
        self.assertEqual(len(self.calls), 1)

    def test_upgrade_data_loss_rejected(self):
        def mutate(command, cwd, argv, output, code, error):
            if command == 'upgrade':
                (cwd / argv[2] / 'local-notes.txt').unlink(missing_ok=True)
            return output, code, error
        self.mutation = mutate
        with self.assertRaisesRegex(checks.VerificationError, 'existing bytes'):
            self.execute()

    def test_rejected_command_mutation_rejected(self):
        def mutate(command, cwd, argv, output, code, error):
            if command == 'doctor':
                (cwd / 'unexpected.txt').write_text('changed')
            return output, code, error
        self.mutation = mutate
        with self.assertRaisesRegex(checks.VerificationError, 'changed files'):
            self.execute()

    def test_unsupported_command_success_rejected(self):
        def mutate(command, cwd, argv, output, code, error):
            return output, (0 if command == 'doctor' else code), error
        self.mutation = mutate
        with self.assertRaisesRegex(checks.VerificationError, 'exit status'):
            self.execute()

    def test_fabricated_execution_rejected(self):
        def mutate(command, cwd, argv, output, code, error):
            if output.get('discovered_checks'):
                output['discovered_checks'][0]['executed'] = True
            return output, code, error
        self.mutation = mutate
        with self.assertRaisesRegex(checks.VerificationError, 'reported as executed'):
            self.execute()

    def test_invalid_diagnostic_rejected(self):
        def mutate(command, cwd, argv, output, code, error):
            return output, code, ('{}' if command == 'doctor' else error)
        self.mutation = mutate
        with self.assertRaisesRegex(checks.VerificationError, 'diagnostic'):
            self.execute()

    def test_timeout_does_not_echo_output(self):
        with patch.object(checks.subprocess, 'run', side_effect=subprocess.TimeoutExpired('x', 60, output='hidden')):
            with self.assertRaises(checks.VerificationError) as error:
                checks.exercise(self.binary, self.root, {}, SOURCES, self.root)
        self.assertNotIn('hidden', str(error.exception))

    def test_invalid_json_does_not_echo_output(self):
        with patch.object(checks.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, 'hidden', '')):
            with self.assertRaises(checks.VerificationError) as error:
                checks.exercise(self.binary, self.root, {}, SOURCES, self.root)
        self.assertNotIn('hidden', str(error.exception))

    def test_false_overall_rejected(self):
        value = audit()
        value['overall'] = 100
        with self.assertRaises(checks.VerificationError):
            checks.audit_is_unexecuted(value)

    def test_missing_testing_field_rejected(self):
        value = audit()
        value['scores'] = {}
        with self.assertRaises(checks.VerificationError):
            checks.audit_is_unexecuted(value)

    def test_false_readiness_rejected(self):
        value = audit()
        value['readiness']['production'] = 100
        with self.assertRaises(checks.VerificationError):
            checks.audit_is_unexecuted(value)

    def test_missing_unexecuted_boundary_rejected(self):
        value = audit()
        value['checks']['not_checked'] = []
        with self.assertRaises(checks.VerificationError):
            checks.audit_is_unexecuted(value)

    def test_fixture_shell_injection_rejected_before_execution(self):
        fixture = checks.load_fixture(self.root)
        fixture['commands'][0].append('; touch sentinel')
        (self.root / checks.FIXTURE).write_text(json.dumps(fixture))
        with self.assertRaisesRegex(checks.VerificationError, 'unreviewed'):
            self.execute()
        self.assertEqual(self.calls, [])

    def test_boolean_fixture_version_rejected(self):
        fixture = checks.load_fixture(self.root)
        fixture['format_version'] = True
        (self.root / checks.FIXTURE).write_text(json.dumps(fixture))
        with self.assertRaises(checks.VerificationError):
            checks.load_fixture(self.root)

    def test_malformed_fixture_rejected(self):
        (self.root / checks.FIXTURE).write_text('{')
        with self.assertRaises(checks.VerificationError):
            checks.load_fixture(self.root)

    def test_read_quick_start_only(self):
        text = '## Quick start\n\n```bash\nah validate ./app\n```\n## Other\n```bash\nnever execute\n```\n'
        self.assertEqual(checks.read_quick_start(text), [['ah', 'validate', './app']])

    def test_multiple_sections_rejected(self):
        with self.assertRaises(checks.VerificationError):
            checks.read_quick_start('## Quick start\n## Quick start with ah\n')

    def test_missing_section_rejected(self):
        with self.assertRaises(checks.VerificationError):
            checks.read_quick_start('')

    def test_bad_quoting_rejected(self):
        with self.assertRaises(checks.VerificationError):
            checks.read_quick_start('## Quick start\n```bash\nah "broken\n```')

    def test_approved_names_and_case(self):
        for name in checks.PUBLIC_NAMES:
            checks.require_public_text(f'https://github.com/powerpuff-kitty/{name.upper()}.git')

    def test_redacted_disallowed_name(self):
        for value in (SYNTHETIC, SYNTHETIC.upper(), SYNTHETIC.replace('-', '%2D')):
            with self.assertRaises(checks.VerificationError) as error:
                checks.require_public_text(value)
            self.assertNotIn(SYNTHETIC, str(error.exception))

    def test_file_content_hash_changes(self):
        generated = self.root / 'generated'
        generated.mkdir()
        file = generated / 'a.txt'
        file.write_text('before')
        before = checks.snapshot(generated)
        file.write_text('after')
        self.assertNotEqual(before, checks.snapshot(generated))

    def test_binary_generated_file_rejected(self):
        generated = self.root / 'generated'
        generated.mkdir()
        (generated / 'a').write_bytes(b'\xff')
        with self.assertRaises(checks.VerificationError):
            checks.snapshot(generated)

    def test_link_generated_file_rejected(self):
        generated = self.root / 'generated'
        generated.mkdir()
        try:
            (generated / 'link').symlink_to(self.binary)
        except (OSError, NotImplementedError):
            self.skipTest('symlinks unavailable')
        with self.assertRaises(checks.VerificationError):
            checks.snapshot(generated)


if __name__ == '__main__':
    unittest.main()
