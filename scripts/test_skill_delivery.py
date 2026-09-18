"""Probe regressions on synthetic files; separate from real candidate execution."""
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import skill_delivery_probe as probe


class SkillDelivery(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.fixture, _ = probe.read_fixture()
        self.project = self.root / 'project'
        (self.project / '.agentic').mkdir(parents=True)
        for name, files in self.fixture['skills'].items():
            for path in files:
                data = ('synthetic document ' + path + '\n').encode()
                target = self.project / '.agents/skills' / name / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(data)
                files[path] = hashlib.sha256(data).hexdigest()
        (self.project / '.agentic/lock.json').write_text(json.dumps({'agents_source':self.fixture['source']}))
        (self.project / '.agentic/THIRD_PARTY_NOTICES.md').write_text(self.fixture['license'], encoding='utf-8')

    def test_reviewed_fixture_has_all_fourteen_files(self):
        fixture, digest = probe.read_fixture()
        self.assertEqual(sum(len(files) for files in fixture['skills'].values()), 14)
        self.assertIn('references/language-review-packs.md', fixture['skills']['codebase-audit'])
        self.assertEqual(len(digest), 64)

    def test_synthetic_exact_payload_is_accepted(self):
        probe.verify_payload(self.project, probe.NAMES, self.fixture)

    def test_missing_changed_and_extra_files_are_rejected(self):
        path = self.project / '.agents/skills/codebase-audit/references/report-template.md'
        original = path.read_bytes()
        path.unlink()
        with self.assertRaises(ValueError):
            probe.verify_payload(self.project, probe.NAMES, self.fixture)
        path.write_bytes(original + b'changed')
        with self.assertRaises(ValueError):
            probe.verify_payload(self.project, probe.NAMES, self.fixture)
        path.write_bytes(original)
        path.with_name('extra.md').write_bytes(b'extra')
        with self.assertRaises(ValueError):
            probe.verify_payload(self.project, probe.NAMES, self.fixture)

    def test_template_readme_is_not_mistaken_for_a_skill(self):
        index = self.project / '.agents/skills/README.md'
        index.write_bytes(b'# Installed agent skills\n\nReusable procedures are installed here from `agentic-harness-agents`. Skills describe how to work; they may not silently redefine canonical project truth or mandatory policy.\n')
        probe.verify_payload(self.project, probe.NAMES, self.fixture)

    def test_unknown_template_index_is_not_silently_ignored(self):
        (self.project / '.agents/skills/README.md').write_bytes(b'unreviewed index')
        with self.assertRaisesRegex(ValueError, 'template index'):
            probe.verify_payload(self.project, probe.NAMES, self.fixture)

    def test_incorrect_pin_is_rejected(self):
        path = self.project / '.agentic/lock.json'
        path.write_text(json.dumps({'agents_source': {'repository':'synthetic','commit':'0'*40}}))
        with self.assertRaisesRegex(ValueError, 'source identity'):
            probe.verify_payload(self.project, probe.NAMES, self.fixture)

    def test_attribution_is_required(self):
        (self.project / '.agentic/THIRD_PARTY_NOTICES.md').write_text('missing')
        with self.assertRaisesRegex(ValueError, 'attribution'):
            probe.verify_payload(self.project, probe.NAMES, self.fixture)

    def test_invalid_fixture_matrix(self):
        original, _ = probe.read_fixture()
        location = self.root / 'fixture.json'
        for field, value in [('format_version', True), ('kind','other'), ('skills',{}), ('license','')]:
            with self.subTest(field=field):
                bad = copy.deepcopy(original)
                bad[field] = value
                location.write_text(json.dumps(bad))
                with patch.object(probe, 'FIXTURE', location), self.assertRaises(ValueError):
                    probe.read_fixture()
        bad = copy.deepcopy(original)
        bad['skills']['codebase-audit']['../outside.md'] = 'a'*64
        location.write_text(json.dumps(bad))
        with patch.object(probe, 'FIXTURE', location), self.assertRaises(ValueError):
            probe.read_fixture()

    def test_pin_mismatch_refuses_before_invocation(self):
        with patch.object(probe.subprocess, 'run') as run, self.assertRaises(ValueError):
            probe.exercise_skill_delivery(self.root / 'absent', self.root, {}, {})
        run.assert_not_called()

    def test_wrong_binary_identity_is_rejected(self):
        executable = self.root / 'synthetic-binary'
        executable.write_text('not executed')
        response = subprocess.CompletedProcess([], 0, json.dumps({'sources':{'agents':{}}}), '')
        with patch.object(probe.subprocess, 'run', return_value=response) as run, self.assertRaises(ValueError):
            probe.exercise_skill_delivery(executable, self.root, {}, self.fixture['source'])
        self.assertEqual(run.call_count, 1)
        self.assertEqual(list(self.root.glob('skill-delivery-*')), [])

    def test_linked_output_is_not_followed(self):
        linked = self.project / '.agents/skills/codebase-audit/references/linked.md'
        try:
            linked.symlink_to(self.project / '.agentic/lock.json')
        except (OSError, NotImplementedError):
            self.skipTest('Symlink creation unavailable')
        with self.assertRaisesRegex(ValueError, 'linked output'):
            probe.verify_payload(self.project, probe.NAMES, self.fixture)


if __name__ == '__main__':
    unittest.main()
