#!/usr/bin/env python3
"""Reviewed onboarding probes; never execute commands extracted from Markdown.

These checks exercise a trusted candidate on disposable synthetic projects. They
are not a general project check runner, privacy scanner, or host sandbox.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import shlex
import subprocess
import tempfile
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = Path('tests/fixtures/public-quick-start.json')
CATALOG_COMMANDS = [
    ['ah', 'init', './my-app', '--boilerplate', 'web-app'],
    ['ah', 'validate', './my-app'],
    ['ah', 'audit', './my-app'],
]
CLI_COMMANDS = [
    ['ah', 'init', './app', '--boilerplate', 'web-app'],
    ['ah', 'audit', './app'],
    ['ah', 'validate', './app'],
]
PUBLIC_NAMES = frozenset({'agentic-harness', 'agentic-harness-agents', 'agentic-harness-cli'})
NAME_PATTERN = re.compile(r'(?<![a-z0-9_-])agentic-harness(?:-[a-z0-9]+)*(?![a-z0-9_-])', re.I)


class VerificationError(RuntimeError):
    """A fixed diagnostic that does not echo candidate or document contents."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


def load_fixture(root: Path = ROOT) -> dict:
    try:
        fixture = json.loads((root / FIXTURE).read_text(encoding='utf-8'))
    except (OSError, UnicodeError, ValueError):
        raise VerificationError('quick-start: unreadable or invalid fixture') from None
    require(isinstance(fixture, dict), 'quick-start: expected an object')
    require(type(fixture.get('format_version')) is int and fixture['format_version'] == 1,
            'quick-start: unsupported format')
    require(fixture.get('kind') == 'public-quick-start', 'quick-start: invalid kind')
    require(fixture.get('commands') == CATALOG_COMMANDS,
            'quick-start: unreviewed command change; execution refused')
    return fixture


def read_quick_start(text: str) -> list[list[str]]:
    sections = re.split(r'^## ', text, flags=re.M)
    selected = [section for section in sections if section.startswith('Quick start')]
    require(len(selected) == 1, 'quick-start: expected one section')
    fences = re.findall(r'^```bash\n(.*?)\n```', selected[0], flags=re.M | re.S)
    require(len(fences) == 1, 'quick-start: expected one bash example')
    try:
        return [shlex.split(line, comments=False) for line in fences[0].splitlines() if line.strip()]
    except ValueError:
        raise VerificationError('quick-start: invalid quoting') from None


def require_public_text(text: str) -> None:
    require(all(match.group().lower() in PUBLIC_NAMES for match in NAME_PATTERN.finditer(unquote(text))),
            'public-surface: unapproved repository reference (value redacted)')


def snapshot(root: Path) -> dict[str, str]:
    """Hash the bounded generated text tree; reject links rather than following them."""
    result = {}
    for path in sorted(root.rglob('*')):
        require(not path.is_symlink(), 'onboarding: unexpected symlink')
        if path.is_dir():
            continue
        require(path.is_file(), 'onboarding: unexpected non-regular file')
        with path.open('rb') as source:
            data = source.read(2_000_001)
        require(len(data) <= 2_000_000, 'onboarding: generated file exceeds fixture bound')
        try:
            text = data.decode('utf-8')
        except UnicodeError:
            raise VerificationError('onboarding: generated non-text file is unsupported') from None
        relative = path.relative_to(root).as_posix()
        require_public_text(relative)
        require_public_text(text)
        result[relative] = hashlib.sha256(data).hexdigest()
    return result


def verify_sources(root: Path = ROOT) -> dict:
    """Check exact source checkouts and public documentation before candidate build."""
    fixture = load_fixture(root)
    lock = json.loads((root / 'upstream.lock.json').read_text(encoding='utf-8'))
    canonical = root / 'upstream/agentic-harness'
    directories = {
        'canonical': canonical,
        'agents': root / 'upstream/agentic-harness-agents',
        'model_registry': root / 'upstream' / ('agentic-harness' + '-registry'),
    }
    for key, directory in directories.items():
        require(not directory.is_symlink(), 'sources: symlink checkout refused')
        require(directory.is_dir(), 'sources: missing checkout')
        head = subprocess.check_output(['git', '-C', str(directory), 'rev-parse', 'HEAD'], text=True).strip()
        require(head == lock[key]['commit'], 'sources: checkout revision mismatch')
        status = subprocess.check_output(['git', '-C', str(directory), 'status', '--porcelain'], text=True)
        require(not status.strip(), 'sources: modified checkout refused')
    # Execute only the explicitly pinned canonical validation module, not discovered scripts.
    public = runpy.run_path(str(canonical / '.github/scripts/validate_public_surface.py'))
    for directory in directories.values():
        require(not public['reference_errors'](directory), 'sources: public text validation failed (details redacted)')
    require(not public['quick_start_errors'](canonical), 'sources: canonical quick-start validation failed')
    actual = json.loads((canonical / 'catalog/quick-start.json').read_text(encoding='utf-8'))
    require(actual == fixture, 'sources: quick-start fixture differs from canonical input')
    require(read_quick_start((root / 'README.md').read_text(encoding='utf-8')) == CLI_COMMANDS,
            'sources: CLI README quick-start drift')
    print('Pinned source identities, public text and both quick starts verified')
    return lock


def audit_is_unexecuted(result: dict) -> None:
    require(result.get('format_version') == 2 and result.get('kind') == 'codebase-audit',
            'onboarding: wrong audit format')
    require('overall' in result and result['overall'] is None, 'onboarding: fabricated overall score')
    require(result.get('scores', {}).get('testing', 'missing') is None,
            'onboarding: fabricated testing score')
    require(result.get('readiness', {}).get('production', 'missing') is None,
            'onboarding: fabricated readiness score')
    require('test execution' in result.get('checks', {}).get('not_checked', []),
            'onboarding: missing test-execution boundary')


def exercise(binary: Path, parent: Path, environment: dict[str, str],
             expected_sources: dict, fixture_root: Path = ROOT) -> dict:
    fixture = load_fixture(fixture_root)
    checks = []
    binary = binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='onboarding-', dir=parent) as temporary:
        work = Path(temporary)

        def invoke(argv: list[str], exit_code: int = 0, diagnostic: bool = False) -> dict:
            try:
                process = subprocess.run([str(binary), *argv], cwd=work, env=environment,
                                         capture_output=True, text=True, encoding='utf-8',
                                         errors='strict', timeout=60, shell=False)
            except (OSError, UnicodeError, subprocess.TimeoutExpired):
                raise VerificationError('onboarding: process failed or timed out (output omitted)') from None
            require(process.returncode == exit_code, 'onboarding: unexpected exit status (output omitted)')
            try:
                result = json.loads(process.stderr if diagnostic else process.stdout)
            except ValueError:
                raise VerificationError('onboarding: invalid JSON response (output omitted)') from None
            require(isinstance(result, dict), 'onboarding: expected JSON object')
            require_public_text(json.dumps(result))
            if diagnostic:
                require(result.get('format_version') == 1 and result.get('kind') == 'diagnostic'
                        and result.get('code') == 'invalid-input', 'onboarding: invalid diagnostic')
            checks.append({'args': argv, 'exit': process.returncode})
            return result

        version = invoke(['--version'])
        require(version.get('sources') == expected_sources, 'onboarding: binary/source pin mismatch')
        for commands in [fixture['commands'], CLI_COMMANDS]:
            for command in commands:
                operation = command[1]
                result = invoke(command[1:], 1 if operation == 'audit' else 0)
                if operation == 'validate':
                    require(result.get('valid') is True, 'onboarding: invalid generated context')
                elif operation == 'audit':
                    audit_is_unexecuted(result)
            snapshot(work / commands[0][2])

        target = work / 'my-app'
        installed_lock = json.loads((target / '.agentic/lock.json').read_text(encoding='utf-8'))
        require(installed_lock.get('canonical_source') == expected_sources['canonical']
                and installed_lock.get('agents_source') == expected_sources['agents'],
                'onboarding: generated lock source mismatch')
        product = target / '.agentic/PRODUCT.md'
        product.write_text(product.read_text(encoding='utf-8') + '\nOwner-approved synthetic product constraint.\n', encoding='utf-8')
        (target / 'local-notes.txt').write_text('Keep this user-authored file.\n', encoding='utf-8')
        (target / 'package.json').write_text(json.dumps({'scripts': {'test': 'synthetic-check-must-not-execute'}}), encoding='utf-8')
        preserved = snapshot(target)
        for _ in range(2):
            invoke(['upgrade', './my-app'])
            current = snapshot(target)
            require(all(current.get(path) == digest for path, digest in preserved.items()),
                    'onboarding: upgrade changed existing bytes')
            require(invoke(['validate', './my-app']).get('valid') is True, 'onboarding: upgrade invalidated context')
        result = invoke(['audit', './my-app'], 1)
        audit_is_unexecuted(result)
        discovered = [check for check in result.get('discovered_checks', []) if check.get('name') == 'test']
        require(len(discovered) == 1 and discovered[0].get('executed') is False,
                'onboarding: discovered check is missing or reported as executed')
        before_rejections = snapshot(work)
        for argv in [
            ['doctor', './my-app'],
            ['new', 'adr', 'Synthetic decision', '--target', './my-app'],
            ['migrate', './my-app'],
            ['migrate', './my-app', '--apply', '--backup', 'backup'],
            ['agentic', 'migrate', './my-app', '--apply'],
        ]:
            invoke(argv, 2, diagnostic=True)
            require(snapshot(work) == before_rejections, 'onboarding: rejected command changed files')
    return {
        'format_version': 1, 'kind': 'onboarding-verification', 'passed': True,
        'version': version, 'fixture_sha256': hashlib.sha256((fixture_root / FIXTURE).read_bytes()).hexdigest(),
        'checks': checks, 'preserved_existing_bytes': True, 'discovered_checks_executed': False,
        'public_generated_text_checked': True,
        'limitations': ['Synthetic context projects, not an application readiness assessment.',
                        'Literal text reference checks do not inspect binary media or Git history.',
                        'Process/network isolation depends on the calling verification environment.'],
    }
