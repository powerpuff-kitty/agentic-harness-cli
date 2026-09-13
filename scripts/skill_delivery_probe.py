"""Verify installed documentation bytes; never execute the installed skills."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'tests/fixtures/flagship-skill-delivery.json'
NAMES = ('codebase-audit', 'design-system-compliance', 'security-review')
PATH = re.compile(r'(?:SKILL\.md|bundle\.json|references/[a-z-]+\.md)\Z')
# Optional index supplied by the pinned base context template, not a skill directory.
INDEX_SHA256 = 'd3d9ab253a85b4af7133f6f7e16a3316e104df066751948497a082ce270d3c94'


def require(ok, message):
    if not ok:
        raise ValueError(message)


def read_fixture():
    data = FIXTURE.read_bytes()
    require(len(data) <= 65536, 'skill-delivery: oversized fixture')
    value = json.loads(data)
    require(set(value) == {'format_version', 'kind', 'source', 'skills', 'license'},
            'skill-delivery: invalid fixture fields')
    require(type(value['format_version']) is int and value['format_version'] == 1
            and value['kind'] == 'flagship-skill-delivery', 'skill-delivery: wrong format')
    require(set(value['source']) == {'repository', 'commit'}
            and value['source']['repository'] == 'powerpuff-kitty/agentic-harness-agents'
            and re.fullmatch(r'[a-f0-9]{40}', value['source']['commit']) is not None,
            'skill-delivery: invalid source')
    require(set(value['skills']) == set(NAMES), 'skill-delivery: unexpected skill selection')
    for files in value['skills'].values():
        require(isinstance(files, dict) and 4 <= len(files) <= 5
                and {'SKILL.md', 'bundle.json', 'references/review-guide.md',
                     'references/report-template.md'} <= set(files), 'skill-delivery: incomplete selection')
        for path, digest in files.items():
            require(PATH.fullmatch(path) is not None and re.fullmatch(r'[a-f0-9]{64}', digest) is not None,
                    'skill-delivery: invalid path or digest')
    require(isinstance(value['license'], str) and value['license'].startswith('MIT License\n')
            and len(value['license']) < 4096, 'skill-delivery: missing attribution')
    return value, hashlib.sha256(data).hexdigest()


def snapshot(root):
    result = {}
    for path in sorted(root.rglob('*')):
        require(not path.is_symlink(), 'skill-delivery: unexpected linked output')
        if path.is_dir():
            continue
        require(path.is_file() and path.stat().st_size <= 2_000_000,
                'skill-delivery: unexpected output type or size')
        result[path.relative_to(root).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    return result


def verify_payload(project, names, fixture):
    skills = project / '.agents/skills'
    installed = {p.name for p in skills.iterdir()}
    if 'README.md' in installed:
        index = skills / 'README.md'
        require(not index.is_symlink() and index.is_file()
                and index.stat().st_size <= 65536
                and hashlib.sha256(index.read_bytes()).hexdigest() == INDEX_SHA256,
                'skill-delivery: unexpected template index')
        installed.remove('README.md')
    require(installed == set(names), 'skill-delivery: unexpected installed skills')
    for name in names:
        require(snapshot(skills / name) == fixture['skills'][name],
                'skill-delivery: missing, changed or extra documentation')
    lock = json.loads((project / '.agentic/lock.json').read_bytes())
    require(lock['agents_source'] == fixture['source'], 'skill-delivery: generated source identity mismatch')
    notice = (project / '.agentic/THIRD_PARTY_NOTICES.md').read_text(encoding='utf-8')
    require(fixture['license'] in notice, 'skill-delivery: attribution not retained')


def exercise_skill_delivery(binary: Path, parent: Path, environment: dict, expected_source: dict):
    fixture, fixture_hash = read_fixture()
    require(fixture['source'] == expected_source, 'skill-delivery: fixture/source pin mismatch')
    binary = binary.resolve(strict=True)
    calls = []
    with tempfile.TemporaryDirectory(prefix='skill-delivery-', dir=parent) as temporary:
        work = Path(temporary)

        def invoke(*argv, expected=0):
            process = subprocess.run([str(binary), *argv], cwd=work, env=environment,
                                     capture_output=True, text=True, encoding='utf-8',
                                     timeout=60, shell=False)
            require(process.returncode == expected, 'skill-delivery: unexpected exit; output omitted')
            value = json.loads(process.stderr if expected == 2 else process.stdout)
            require(isinstance(value, dict), 'skill-delivery: expected structured output')
            if expected == 2:
                require(value.get('kind') == 'diagnostic', 'skill-delivery: missing diagnostic')
            calls.append({'args': list(argv), 'exit': process.returncode})
            return value

        version = invoke('--version')
        require(version['sources']['agents'] == expected_source, 'skill-delivery: binary/source mismatch')
        for name in NAMES:
            invoke('init', name, '--boilerplate', 'base', '--skill', name)
            verify_payload(work / name, (name,), fixture)
            require(invoke('validate', name).get('valid') is True, 'skill-delivery: invalid single-skill project')

        flags = [arg for name in NAMES for arg in ('--skill', name)]
        invoke('init', 'combined', '--boilerplate', 'base', *flags)
        project = work / 'combined'
        verify_payload(project, NAMES, fixture)
        require(invoke('validate', 'combined').get('valid') is True, 'skill-delivery: invalid combined project')
        local = project / '.agents/skills/codebase-audit/references/review-guide.md'
        local.write_bytes(local.read_bytes() + b'\nOwner-approved synthetic local guidance.\n')
        (project / 'owner-notes.txt').write_bytes(b'Keep this synthetic user-owned note.\n')
        preserved = snapshot(project)
        for _ in range(2):
            invoke('upgrade', 'combined', '--boilerplate', 'base', *flags)
            after = snapshot(project)
            require(all(after.get(p) == digest for p, digest in preserved.items()),
                    'skill-delivery: upgrade changed existing bytes')
            require(invoke('validate', 'combined').get('valid') is True, 'skill-delivery: upgrade invalidated project')

        # Only an owned disposable test file is removed; the CLI must restore the missing reference.
        missing = project / '.agents/skills/design-system-compliance/references/report-template.md'
        missing.unlink()
        invoke('upgrade', 'combined', '--boilerplate', 'base', '--skill', 'design-system-compliance')
        require(snapshot(project) == preserved, 'skill-delivery: reference recovery changed preserved inputs')
        require(invoke('validate', 'combined').get('valid') is True, 'skill-delivery: recovered project invalid')
        before = snapshot(work)
        invoke('upgrade', 'combined', '--skill', 'unknown-synthetic-skill', expected=2)
        require(snapshot(work) == before, 'skill-delivery: invalid selection mutated project')
    return {'format_version': 1, 'kind': 'skill-delivery-verification', 'passed': True,
            'source': expected_source, 'fixture_sha256': fixture_hash, 'checks': calls,
            'skills': list(NAMES), 'references_verified': True, 'existing_bytes_preserved': True,
            'model_execution': 'not-run', 'host_delivery_verified': False,
            'limitations': ['Synthetic installation and preservation, not skill reasoning quality.',
                           'Customized preserved files need review; they are not certified as upstream-identical.']}
