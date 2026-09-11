#!/usr/bin/env python3
"""Produce/verify checksums and reproducible build-input identity alongside an asset."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
p = argparse.ArgumentParser()
p.add_argument('binary', type=Path)
p.add_argument('--commit')
p.add_argument('--verify', action='store_true')
a = p.parse_args()
binary = a.binary.resolve(strict=True)
digest = hashlib.sha256(binary.read_bytes()).hexdigest()
checksum = Path(str(binary) + '.sha256')
provenance = Path(str(binary) + '.provenance.json')
expected = f'{digest}  {binary.name}\n'
version = json.loads(subprocess.check_output([str(binary), '--version']))
if a.verify:
    assert checksum.read_text() == expected, 'checksum mismatch'
    evidence = json.loads(provenance.read_text())
    assert evidence['sha256'] == digest and evidence['version'] == version, 'provenance mismatch'
else:
    assert a.commit, '--commit is required when packaging'
    checksum.write_text(expected)
    provenance.write_text(json.dumps({'format_version': 1, 'asset': binary.name, 'sha256': digest,
                                     'commit': a.commit, 'version': version,
                                     'toolchain': subprocess.check_output(['rustc', '--version'], text=True).strip()}, indent=2) + '\n')
