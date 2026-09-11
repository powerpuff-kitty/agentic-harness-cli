#!/usr/bin/env python3
"""Exercise a copied candidate offline, outside the checkout, using only Python stdlib."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument('binary', type=Path)
parser.add_argument('--report', type=Path)
args = parser.parse_args()
source = args.binary.resolve(strict=True)
results = []
with tempfile.TemporaryDirectory(prefix='ah-candidate-') as directory:
    root = Path(directory)
    binary = root / ('ah.exe' if os.name == 'nt' else 'ah')
    shutil.copy2(source, binary)
    environment = os.environ.copy()
    environment.pop('AH_REGISTRY', None)
    environment['PATH'] = str(root)  # No Cargo, sibling binaries, or other runtime dependencies.
    environment.update(HTTP_PROXY='http://127.0.0.1:1', HTTPS_PROXY='http://127.0.0.1:1')

    def run(*argv, exits=(0,), json_output=True):
        process = subprocess.run([str(binary), *argv], cwd=root, env=environment,
                                 capture_output=True, text=True, timeout=60)
        assert process.returncode in exits, (argv, process.returncode, process.stdout, process.stderr)
        results.append({'args': argv, 'exit': process.returncode})
        return json.loads(process.stdout) if json_output else process.stdout

    version = run('--version')
    for family in [[], ['architecture'], ['design'], ['agentic']]:
        run(*family, '--help', json_output=False)
    run('catalog-check')
    run('init', 'project', '--boilerplate', 'web-app')
    run('validate', 'project')
    run('harness-audit', 'project')
    run('upgrade', 'project')
    run('validate', 'project')
    # Restore a project backup and validate it, as the first-release recovery rehearsal.
    shutil.copytree(root / 'project', root / 'backup')
    run('upgrade', 'project', '--policy', 'licensing')
    shutil.rmtree(root / 'project')
    shutil.copytree(root / 'backup', root / 'project')
    run('validate', 'project')
    run('security-scan', 'project')
    run('design-system-components', 'project', '--write')
    for command in ['detect', 'analyze', 'enforce']:
        run('architecture', command, 'project')
    run('architecture', 'enforce', 'project', '--write')
    result = run('audit', 'project', exits=(0, 1))
    assert result['overall'] is None
    (root / 'audit.json').write_text(json.dumps(result))
    run('compare', 'audit.json', 'audit.json')
    run('gate', 'audit.json', '--min-overall', '90', exits=(1,))
    run('gate', 'audit.json', '--max-architecture-errors', '0')
    # Exercise the bundled parser worker with no runtime tools on PATH.
    (root / 'source').mkdir()
    (root / 'source' / 'env.ts').write_text('export interface Box<T> { value: T }')
    (root / 'source' / 'main.ts').write_text(
        "export type App = import('./env').Box<{ value: string }>;")
    parsed = run('architecture', 'analyze', 'source')
    assert parsed['compliance']['complete'] is True
    assert len(parsed['graph']['edges']) == 1
    assert parsed['graph']['edges'][0]['kind'] == 'type'
    (root / 'source' / 'bad.ts').write_text('const broken: = ;')
    partial = run('architecture', 'analyze', 'source')
    assert partial['compliance']['complete'] is False
    assert partial.get('score') is None
    for command in ['audit', 'context', 'skills', 'improve']:
        run('agentic', command, 'project')
    models = run('agentic', 'models', 'project')['models']
    assert len(models) >= 2
    ids = [m.get('id') or m.get('model_id') for m in models]
    run('agentic', 'migrate', 'project', '--from', ids[0], '--to', ids[1])
    run('agentic', 'compare', ids[0], ids[1])
    run('design', 'analyze', 'project', '--level', 'static', '--output', 'analysis.json')
    run('design', 'preserve', '--analysis', 'analysis.json', '--output', 'genome.json')
    run('design', 'diff', 'analysis.json', 'analysis.json', '--output', 'diff.json')
    for argv in [('validate', 'absent'), ('audit', 'project', '--invalid'),
                 ('agentic', 'improve', 'project', '--apply')]:
        run(*argv, exits=(2,), json_output=False)

report = {'format_version': 1, 'kind': 'candidate-verification', 'passed': True,
          'binary_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
          'version': version, 'checks': results, 'recovery': 'backup/restore validated',
          'limitations': ['Network access is not OS-sandboxed; proxy variables deny ordinary HTTP clients.',
                          'Design prompt approved-artifact loop is exercised by Rust integration tests.']}
text = json.dumps(report, indent=2) + '\n'
if args.report:
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(text)
print(text)
