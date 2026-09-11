#!/usr/bin/env python3
"""Exercise the POSIX installer and source launcher using a temporary custom prefix."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
p = argparse.ArgumentParser()
p.add_argument('binary', type=Path)
a = p.parse_args()
binary = a.binary.resolve(strict=True)
root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='ah-install-') as directory:
    prefix = Path(directory)
    subprocess.run(['sh', str(root / 'install.sh'), '--prefix', str(prefix), '--command', 'custom-ah', '--binary', str(binary)], check=True)
    installed = prefix / 'bin/custom-ah'
    for executable in [installed, root / 'ah']:
        for family in [[], ['architecture'], ['design'], ['agentic']]:
            subprocess.run([str(executable), *family, '--help'], cwd=prefix, stdout=subprocess.DEVNULL, check=True)
    assert json.loads(subprocess.check_output([str(installed), '--version'])) == json.loads(subprocess.check_output([str(binary), '--version']))
    before = installed.read_bytes()
    invalid = prefix / 'invalid-binary'
    invalid.write_text('#!/bin/sh\nexit 2\n')
    failed = subprocess.run(['sh', str(root / 'install.sh'), '--prefix', str(prefix), '--command', 'custom-ah', '--binary', str(invalid)], capture_output=True)
    assert failed.returncode != 0 and installed.read_bytes() == before
    invalid_name = subprocess.run(['sh', str(root / 'install.sh'), '--prefix', str(prefix), '--command', '../escape', '--binary', str(binary)], capture_output=True)
    assert invalid_name.returncode == 2
print('Custom-prefix installation, failed-install preservation and source launcher passed')
