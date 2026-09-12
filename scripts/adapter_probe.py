"""Exercise context-only installation on synthetic projects, never a live host.

The optional validator checks actual command outputs against the pinned schema.
All executable paths are supplied by the existing trusted-candidate scripts.
"""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def digest(data: bytes) -> str:
    return 'sha256:' + hashlib.sha256(data).hexdigest()


def snapshot(root: Path) -> dict:
    state = {}
    for path in sorted(root.rglob('*')):
        if path.is_symlink():
            raise AssertionError('adapter probe: unexpected link')
        state[path.relative_to(root).as_posix()] = (
            None if path.is_dir() else digest(path.read_bytes())
        )
    return state


def exercise_adapters(binary: Path, parent: Path, environment: dict,
                      expected_source: dict, validate=None) -> dict:
    binary = binary.resolve(strict=True)
    calls = []
    with tempfile.TemporaryDirectory(prefix='adapter-probes-', dir=parent) as directory:
        work = Path(directory)

        def fixture(name):
            root = work / name
            (root / 'context').mkdir(parents=True)
            (root / 'AGENTS.md').write_bytes(b'# Project instructions\nRead context/architecture.md.\n')
            (root / 'context/architecture.md').write_bytes(b'Keep approved synthetic project rules.\n')
            return root

        def invoke(root, args, expected=0):
            result = subprocess.run([str(binary), 'adapters', 'sync', *args],
                                    cwd=root, env=environment, capture_output=True,
                                    text=True, encoding='utf-8', timeout=60, shell=False)
            if result.returncode != expected:
                raise AssertionError('adapter probe: unexpected exit status (output omitted)')
            output = json.loads(result.stderr if expected == 2 else result.stdout)
            calls.append({'args': ['adapters', 'sync', *args], 'exit': result.returncode})
            if expected == 2:
                assert not result.stdout
                assert output['kind'] == 'diagnostic' and output['code'] == 'invalid-input'
                return output
            assert output['format_version'] == 1 and output['kind'] == 'adapter-sync'
            assert output['host_delivery_verified'] is False
            assert output['enforcement_verified'] is False
            assert output['source'] == expected_source
            assert output['router_sha256'] == digest((root / 'AGENTS.md').read_bytes())
            if validate is not None:
                validate(output)
            if output['operation'] == 'preview':
                unsigned = {key: value for key, value in output.items() if key != 'plan_digest'}
                encoded = json.dumps(unsigned, sort_keys=True, ensure_ascii=False,
                                     separators=(',', ':')).encode('utf-8')
                framed = b'ah-adapter-sync-v1\0' + len(encoded).to_bytes(8, 'big') + encoded
                assert output['plan_digest'] == digest(framed)
            return output

        def select(host, profile='base'):
            return ['--host', host, '--profile', profile]

        def apply(root, options, preview, expected=0):
            result = invoke(root, [*options, '--apply', '--review', preview['plan_digest']], expected)
            if expected == 0:
                assert result['status'] == 'applied'
                assert result['plan_digest'] == preview['plan_digest']
                assert result['entries'] == preview['entries']
                for entry in result['entries']:
                    assert digest((root / entry['target']).read_bytes()) == entry['desired_sha256']
            return result

        # First installation, source preservation and repeated no-op application.
        root = fixture('claude')
        (root / '.claude').mkdir()
        (root / '.claude/settings.json').write_bytes(b'{"synthetic_custom_setting":true}\n')
        initial = snapshot(root)
        options = select('claude')
        preview = invoke(root, options)
        assert preview == invoke(root, options)
        assert initial == snapshot(root)
        apply(root, options, preview)
        assert (root / 'CLAUDE.md').read_bytes() == b'@AGENTS.md\n'
        assert b'MIT License' in (root / '.agents/adapters/LICENSE').read_bytes()
        assert not (root / '.claude/rules').exists()
        before_repeat = snapshot(root)
        repeat = invoke(root, options)
        repeated = apply(root, options, repeat)
        assert repeated['created_files'] == [] and repeated['created_directories'] == []
        assert before_repeat == snapshot(root)
        options = select('claude', 'typed-ui')
        apply(root, options, invoke(root, options))
        assert (root / '.claude/rules/agentic-typed-ui.md').is_file()
        current = snapshot(root)
        assert all(current.get(path) == value for path, value in initial.items())
        assert not (root / '.agentic').exists()

        # Scoped Cursor installation and native no-file host routes.
        root = fixture('cursor-scoped')
        options = select('cursor', 'typed-ui')
        apply(root, options, invoke(root, options))
        assert (root / '.cursor/rules/agentic-typed-ui.mdc').is_file()
        assert not (root / 'CLAUDE.md').exists()
        for host in ['cursor', 'codex']:
            root = fixture('native-' + host)
            before = snapshot(root)
            options = select(host)
            result = apply(root, options, invoke(root, options))
            assert result['entries'] == [] and before == snapshot(root)

        # A known conflict blocks the complete batch, including notice creation.
        root = fixture('conflict')
        (root / 'CLAUDE.md').write_bytes(b'Owner-authored instructions; do not replace.\n')
        before = snapshot(root)
        options = select('claude', 'typed-ui')
        preview = invoke(root, options, 1)
        conflict = apply(root, options, preview, 1)
        assert conflict['status'] == 'conflict' and conflict['created_files'] == []
        assert before == snapshot(root)
        assert not (root / '.agents').exists()

        # Changed router or destination invalidates review without side effects.
        for name in ['AGENTS.md', 'CLAUDE.md']:
            root = fixture('stale-' + name)
            options = select('claude')
            preview = invoke(root, options)
            (root / name).write_bytes(b'Changed by project owner.\n')
            before = snapshot(root)
            apply(root, options, preview, 2)
            assert before == snapshot(root)

        root = fixture('invalid')
        before = snapshot(root)
        invoke(root, ['--host', 'claude', '--force'], 2)
        assert before == snapshot(root)
    return {'format_version': 1, 'kind': 'adapter-installation-verification',
            'passed': True, 'source': expected_source, 'checks': calls,
            'host_delivery_verified': False, 'enforcement_verified': False,
            'limitations': ['Synthetic filesystem installation, not native host execution.',
                            'No managed replacement, removal or hostile-race guarantee.']}
