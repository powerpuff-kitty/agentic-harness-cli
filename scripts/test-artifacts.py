#!/usr/bin/env python3
"""Regression checks for attribution coverage and untrusted candidate verification."""
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


artifacts = load('package-candidate')
notices = load('dependency-notices')
BINARY, NOTICES = map(lambda p: Path(p).resolve(), sys.argv[1:3])
sys.argv = sys.argv[:1]


class ArtifactTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.candidate = tempfile.TemporaryDirectory()
        cls.original = Path(cls.candidate.name) / BINARY.name
        shutil.copy2(BINARY, cls.original)
        artifacts.package(cls.original, '0' * 40, NOTICES)

    @classmethod
    def tearDownClass(cls):
        cls.candidate.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        shutil.copytree(self.candidate.name, self.root, dirs_exist_ok=True)
        self.binary = self.root / BINARY.name

    def test_complete_bundle_is_repeatable_and_verifies(self):
        archive = Path(str(self.binary) + '.zip')
        before = archive.read_bytes()
        artifacts.package(self.binary, '0' * 40, NOTICES)
        self.assertEqual(before, archive.read_bytes())
        artifacts.verify(self.binary)
        artifacts.verify_archive(archive)

    def test_corrupt_binary_is_rejected_before_execution(self):
        self.binary.write_bytes(b'corrupted executable')
        with patch.object(artifacts.subprocess, 'check_output') as execute:
            with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
                artifacts.verify(self.binary)
            execute.assert_not_called()

    def test_integrity_checks_remain_active_with_python_optimization(self):
        self.binary.write_bytes(b'corrupted executable')
        result = subprocess.run([sys.executable, '-O', str(Path(__file__).with_name('package-candidate.py')),
                                 str(self.binary), '--verify'], capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn('checksum mismatch', result.stderr)
        self.assertEqual(result.stdout, '')

    def test_missing_or_modified_attribution_prevents_execution(self):
        for suffix in ['.notices.txt', '.notices.json', '.notices.rust.html']:
            with self.subTest(suffix=suffix):
                path = Path(str(self.binary) + suffix)
                original = path.read_bytes()
                path.write_bytes(original + b'changed')
                with patch.object(artifacts.subprocess, 'check_output') as execute:
                    with self.assertRaisesRegex(ValueError, 'attribution checksum mismatch'):
                        artifacts.verify(self.binary)
                    execute.assert_not_called()
                path.unlink()
                with self.assertRaises(FileNotFoundError):
                    artifacts.verify(self.binary, execute=False)
                path.write_bytes(original)

    def test_source_licensing_is_a_separate_release_gate(self):
        path = Path(str(self.binary) + '.notices.json')
        report = json.loads(path.read_text())
        report['missing_authored_licenses'] = ['fixture: no owner declaration']
        path.write_text(json.dumps(report), encoding='utf-8')
        provenance = Path(str(self.binary) + '.provenance.json')
        evidence = json.loads(provenance.read_text())
        evidence['attribution'][path.name] = artifacts.digest(path)
        provenance.write_text(json.dumps(evidence), encoding='utf-8')
        artifacts.verify(self.binary, execute=False)
        with self.assertRaisesRegex(ValueError, 'licensing remains unresolved'):
            artifacts.verify(self.binary, execute=False, require_authored=True)

    def test_archive_cannot_introduce_extra_or_traversing_members(self):
        archive = Path(str(self.binary) + '.zip')
        with zipfile.ZipFile(archive, 'a') as bundle:
            bundle.writestr('../outside', 'untrusted')
        Path(str(archive) + '.sha256').write_text(f'{artifacts.digest(archive)}  {archive.name}\n')
        with self.assertRaisesRegex(ValueError, 'unexpected archive members'):
            artifacts.verify_archive(archive)


class NoticeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.package = {'name': 'fixture', 'version': '1.0', 'license': 'MIT',
                        'manifest_path': str(self.root / 'Cargo.toml'), 'source': 'registry+fixture',
                        'repository': 'https://example.invalid/fixture'}

    def test_missing_license_text_is_not_satisfied_by_spdx_metadata(self):
        with self.assertRaisesRegex(ValueError, 'no attribution text'):
            notices.collect([self.package], [], self.root)

    def test_exact_source_text_and_nested_notices_are_retained(self):
        for newline in ['\n', '\r\n']:
            with self.subTest(newline=repr(newline)):
                text = f'Copyright fixture{newline}UTF-8 attribution: é{newline}'
                (self.root / 'LICENSE').write_bytes(text.encode('utf-8'))
                nested = self.root / 'src/unicode/LICENSE-UNICODE'
                nested.parent.mkdir(parents=True, exist_ok=True)
                nested.write_bytes(b'Additional Unicode attribution\n')
                records, sections = notices.collect([self.package], [], self.root)
                self.assertEqual(len(records[0]['files']), 2)
                self.assertIn(text, ''.join(sections))
                self.assertEqual(records[0]['files'][0]['sha256'], notices.digest(text.encode()))

    def test_supplement_requires_matching_version_commit_and_bytes(self):
        supplement = self.root / 'supplement'
        supplement.mkdir()
        data = b'upstream license text\n'
        (supplement / 'upstream.txt').write_bytes(data)
        (self.root / '.cargo_vcs_info.json').write_text(json.dumps({'git': {'sha1': 'a' * 40}}))
        source = {'packages': [{'name': 'fixture', 'version': '1.0'}], 'commit': 'a' * 40,
                  'repository': self.package['repository'], 'file': 'upstream.txt',
                  'source': 'https://example.invalid/pinned/LICENSE', 'sha256': notices.digest(data)}
        records, _ = notices.collect([self.package], [source], supplement)
        self.assertEqual(len(records), 1)
        source['commit'] = 'b' * 40
        with self.assertRaisesRegex(ValueError, 'does not match pinned crate'):
            notices.collect([self.package], [source], supplement)
        source['commit'] = 'a' * 40
        (supplement / 'upstream.txt').write_bytes(b'modified')
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            notices.collect([self.package], [source], supplement)


if __name__ == '__main__':
    unittest.main()
