#!/usr/bin/env python3
"""Probe a supplied ah binary's audit readers, not only its library functions.

All commands and inputs are synthetic and fixed by the local test corpus. The
binary is copied outside its checkout and gets an isolated temporary home. No
repository scripts, network/provider commands or user-project writes are invoked.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / 'tests/fixtures/audit-command-v1.json'
LIMIT = 1_048_576


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fixture_bytes(record: dict) -> bytes:
    if record['encoding'] == 'utf-8':
        data = record['input'].encode('utf-8')
    elif record['encoding'] == 'hex':
        data = bytes.fromhex(record['input'])
    else:
        raise ValueError('unsupported-fixture-encoding')
    if len(data) > 4096 or sha256(data) != record['sha256']:
        raise ValueError('fixture-identity-mismatch')
    return data


def load_corpus() -> dict:
    with CORPUS.open('rb') as handle:
        data = handle.read(LIMIT + 1)
    if len(data) > LIMIT:
        raise ValueError('oversized-corpus')
    value = json.loads(data)
    if value.get('format_version') != 1 or value.get('kind') != 'audit-command-conformance':
        raise ValueError('unsupported-corpus')
    if not 0 < len(value['cases']) <= 128 or not 0 < len(value['fixtures']) <= 64:
        raise ValueError('corpus-count-limit')
    for ident, record in value['fixtures'].items():
        if not ident or not all(c in 'abcdefghijklmnopqrstuvwxyz0123456789-' for c in ident):
            raise ValueError('invalid-fixture-id')
        fixture_bytes(record)
    ids = set()
    for case in value['cases']:
        if case['id'] in ids:
            raise ValueError('duplicate-case-id')
        ids.add(case['id'])
        if case['command'] not in ('gate', 'compare'):
            raise ValueError('unsupported-command')
        needed = 1 if case['command'] == 'gate' else 2
        if len(case['inputs']) != needed or any(n not in value['fixtures'] for n in case['inputs']):
            raise ValueError('invalid-command-inputs')
        options = case['options']
        if case['command'] == 'compare' and options:
            raise ValueError('unexpected-compare-options')
        if len(options) % 2 or any(options[i] not in ('--min-overall', '--min-score', '--max-architecture-errors')
                                   for i in range(0, len(options), 2)):
            raise ValueError('unsupported-gate-options')
        if any(not isinstance(option, str) or len(option) > 128 for option in options):
            raise ValueError('invalid-command-option')
        if type(case['expected']['exit_code']) is not int or case['expected']['exit_code'] not in (0, 1, 2):
            raise ValueError('invalid-expected-exit')
    return value


def _field(value: dict, path: str) -> tuple[bool, object]:
    current = value
    for part in path.split('.'):
        if not isinstance(current, dict) or part not in current:
            return False, None
        current = current[part]
    return True, current


def _equal(actual: object, expected: object) -> bool:
    if type(expected) in (int, float):
        return type(actual) in (int, float) and actual == expected
    return type(actual) is type(expected) and actual == expected


def mismatches(case: dict, output: subprocess.CompletedProcess) -> list[str]:
    expected = case['expected']
    errors = []
    if output.returncode != expected['exit_code']:
        errors.append('exit-code')
    rejected = expected['exit_code'] == 2
    payload = output.stderr if rejected else output.stdout
    silent = output.stdout if rejected else output.stderr
    if silent.strip():
        errors.append('unexpected-output-channel')
    try:
        result = json.loads(payload)
    except (ValueError, UnicodeError):
        return errors + ['missing-structured-result']
    if not isinstance(result, dict):
        return errors + ['non-object-result']
    if type(result.get('format_version')) is not int or result['format_version'] != 1:
        errors.append('format-version')
    if result.get('kind') != expected['kind']:
        errors.append('result-kind')
    for path, wanted in expected['fields'].items():
        present, actual = _field(result, path)
        if not present or not _equal(actual, wanted):
            errors.append('field:' + path)
    if case['group'] == 'duplicate' and b'private-fixture-marker' in output.stderr:
        errors.append('input-echoed-in-diagnostic')
    return errors


def isolated_environment(root: Path) -> dict[str, str]:
    result = {'HOME': str(root), 'USERPROFILE': str(root), 'TMPDIR': str(root),
              'TMP': str(root), 'TEMP': str(root), 'LANG': 'C.UTF-8', 'LC_ALL': 'C.UTF-8'}
    # Required by Windows native process creation; provider credentials and user
    # shell configuration are intentionally not inherited.
    for name in ('SystemRoot', 'WINDIR'):
        if name in os.environ:
            result[name] = os.environ[name]
    return result


def probe(binary: Path, corpus: dict) -> dict:
    binary = binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='ah-audit-command-') as directory:
        root = Path(directory)
        copied = root / ('ah.exe' if os.name == 'nt' else 'ah')
        shutil.copy2(binary, copied)
        copied.chmod(0o700)
        binary_digest = sha256(copied.read_bytes())
        for name, record in corpus['fixtures'].items():
            (root / (name + '.json')).write_bytes(fixture_bytes(record))
        env = isolated_environment(root)
        observations = []
        for case in corpus['cases']:
            argv = [case['command'], *(n + '.json' for n in case['inputs']), *case['options']]
            try:
                output = subprocess.run([str(copied), *argv], cwd=root, env=env,
                                        capture_output=True, timeout=10, check=False)
                errors = mismatches(case, output)
                observations.append({'id': case['id'], 'group': case['group'], 'argv': argv,
                                     'expected_exit': case['expected']['exit_code'],
                                     'observed_exit': output.returncode, 'matched': not errors,
                                     'mismatches': errors,
                                     'stdout': output.stdout[:4096].decode('utf-8', 'replace'),
                                     'stderr': output.stderr[:4096].decode('utf-8', 'replace')})
            except subprocess.TimeoutExpired:
                observations.append({'id': case['id'], 'group': case['group'], 'argv': argv,
                                     'matched': False, 'mismatches': ['command-timeout']})
        inputs_preserved = all((root / (n + '.json')).read_bytes() == fixture_bytes(r)
                               for n, r in corpus['fixtures'].items())
        binary_preserved = sha256(copied.read_bytes()) == binary_digest
        matched = sum(o['matched'] for o in observations)
        return {'format_version': 1, 'kind': 'audit-command-probe',
                'scope': 'synthetic-gate-and-compare-dispatch', 'binary_sha256': binary_digest,
                'total': len(observations), 'matched': matched,
                'passed': matched == len(observations) and inputs_preserved and binary_preserved,
                'input_bytes_preserved': inputs_preserved, 'binary_bytes_preserved': binary_preserved,
                'completion_verified': False, 'release_verified': False,
                'results': observations, 'limitations': corpus['limitations']}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    try:
        result = probe(args.binary, load_corpus())
        text = json.dumps(result, indent=2, ensure_ascii=False, allow_nan=False) + '\n'
        if args.report:
            # Do not silently replace an earlier evidence record.
            with args.report.open('x', encoding='utf-8', newline='\n') as handle:
                handle.write(text)
        print(json.dumps({k: v for k, v in result.items() if k != 'results'}, indent=2))
        return 0 if result['passed'] else 1
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f'audit-command-probe: {type(error).__name__}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
