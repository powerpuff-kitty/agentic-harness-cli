import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from source_bytes import ADAPTER_FILES, clone_source, verify_adapter_bytes


class SourceBytesTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        config = self.root / 'gitconfig'
        config.write_text('[core]\n\tautocrlf = true\n', encoding='utf-8')
        self.environment = patch.dict(os.environ, {
            'GIT_CONFIG_GLOBAL': str(config), 'GIT_CONFIG_NOSYSTEM': '1'})
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.source = self.root / 'source'
        self.source.mkdir()
        self.git('init', '-q')
        self.git('config', 'user.name', 'Synthetic fixture')
        self.git('config', 'user.email', 'fixture@example.invalid')
        self.git('config', 'core.autocrlf', 'false')
        for relative in ADAPTER_FILES:
            path = self.source / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'First line\nSecond line\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'Synthetic adapter bytes')

    def git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.source), *args],
                                       stderr=subprocess.DEVNULL)

    def checkout(self):
        target = self.root / 'checkout'
        clone_source(str(self.source), target)
        subprocess.run(['git', '-C', str(target), 'checkout', '--quiet', '--detach', 'HEAD'],
                       check=True)
        return target

    def test_clone_preserves_lf_despite_host_autocrlf(self):
        target = self.checkout()
        verify_adapter_bytes(target)
        self.assertEqual((target / ADAPTER_FILES[2]).read_bytes(), b'First line\nSecond line\n')
        self.assertEqual(subprocess.check_output(
            ['git', '-C', str(target), 'config', '--local', 'core.autocrlf']).strip(), b'false')
        self.assertIn('autocrlf = true', (self.root / 'gitconfig').read_text())

    def test_crlf_checkout_is_rejected_not_rewritten(self):
        target = self.checkout()
        path = target / ADAPTER_FILES[2]
        changed = path.read_bytes().replace(b'\n', b'\r\n')
        path.write_bytes(changed)
        with self.assertRaisesRegex(RuntimeError, 'pinned Git bytes'):
            verify_adapter_bytes(target)
        self.assertEqual(path.read_bytes(), changed)

    def test_modified_input_fails_without_logging_contents(self):
        target = self.checkout()
        (target / ADAPTER_FILES[0]).write_bytes(b'unrelated-private-content-example')
        with self.assertRaises(RuntimeError) as error:
            verify_adapter_bytes(target)
        self.assertNotIn('private-content-example', str(error.exception))

    def test_missing_input_fails(self):
        target = self.checkout()
        (target / ADAPTER_FILES[1]).unlink()
        with self.assertRaises(RuntimeError):
            verify_adapter_bytes(target)

    def test_existing_clone_target_is_not_replaced(self):
        target = self.root / 'owned-target'
        target.mkdir()
        (target / 'notes').write_bytes(b'preserve')
        with self.assertRaises(subprocess.CalledProcessError):
            clone_source(str(self.source), target)
        self.assertEqual((target / 'notes').read_bytes(), b'preserve')


if __name__ == '__main__':
    unittest.main()
