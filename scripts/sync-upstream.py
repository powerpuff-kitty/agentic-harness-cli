#!/usr/bin/env python3
"""Fetch exact build inputs without deleting existing checkouts or local edits."""
import json
from pathlib import Path
import re
import subprocess
import sys
from onboarding import verify_sources
from source_bytes import clone_source, verify_adapter_bytes

root = Path(__file__).resolve().parents[1]
lock = json.loads((root / 'upstream.lock.json').read_text())
for key, directory in [('canonical', 'agentic-harness'), ('agents', 'agentic-harness-agents'),
                       ('model_registry', 'agentic-harness-registry')]:
    entry = lock[key]
    commit, repository = entry['commit'], entry['repository']
    if not re.fullmatch(r'[0-9a-f]{40}', commit) or not re.fullmatch(r'powerpuff-kitty/[a-z-]+', repository):
        raise SystemExit(f'Invalid locked source: {key}')
    path = root / 'upstream' / directory
    url = f'https://github.com/{repository}.git'
    if path.exists():
        if path.is_symlink():
            raise SystemExit(f'Refusing symlink source: {path}')
        actual = subprocess.check_output(['git', '-C', str(path), 'remote', 'get-url', 'origin'], text=True).strip()
        if actual != url:
            raise SystemExit(f'Unexpected source remote for {key}: {actual}')
        status = subprocess.check_output(['git', '-C', str(path), 'status', '--porcelain'], text=True)
        if status:
            raise SystemExit(f'Local source edits in {path}; preserve or commit them before sync')
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        clone_source(url, path)
    subprocess.run(['git', '-C', str(path), 'fetch', '--quiet', 'origin', commit], check=True)
    subprocess.run(['git', '-C', str(path), 'checkout', '--quiet', '--detach', commit], check=True)
    actual = subprocess.check_output(['git', '-C', str(path), 'rev-parse', 'HEAD'], text=True).strip()
    assert actual == commit
    print(f'{key}={actual}')

verify_sources(root)
verify_adapter_bytes(root / 'upstream/agentic-harness-agents')
subprocess.run([sys.executable, '-m', 'unittest', 'discover', '-s', str(root / 'scripts'),
                '-p', 'test_source_bytes.py'], check=True)
print('Adapter payload bytes match pinned Git objects; clone regression tests passed')
