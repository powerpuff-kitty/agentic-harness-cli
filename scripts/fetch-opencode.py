#!/usr/bin/env python3
"""Fetch the pinned Linux CI host binary after verifying its archive integrity."""
import argparse
import base64
import hashlib
import io
import json
from pathlib import Path
import tarfile
import urllib.request

ROOT=Path(__file__).resolve().parents[1]

def fetch(output):
    lock=json.loads((ROOT/'catalog/opencode-host.json').read_text())
    with urllib.request.urlopen(lock['url'],timeout=60) as response:data=response.read(160_000_001)
    if len(data)>160_000_000:raise ValueError('Host archive exceeds size bound')
    actual='sha512-'+base64.b64encode(hashlib.sha512(data).digest()).decode()
    if actual!=lock['integrity']:raise ValueError('Host archive integrity mismatch')
    with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as archive:
        members=archive.getmembers()
        if len(members)>20:raise ValueError('Unexpected host archive inventory')
        matching=[m for m in members if m.name==lock['member']]
        if len(matching)!=1 or not matching[0].isfile() or matching[0].size>600_000_000:raise ValueError('Invalid host binary member')
        stream=archive.extractfile(matching[0]);binary=stream.read(600_000_001)
        if len(binary)!=matching[0].size:raise ValueError('Incomplete host binary')
    output=Path(output)
    if output.exists() or output.is_symlink():raise ValueError('Choose a new host binary destination')
    output.parent.mkdir(parents=True,exist_ok=True);output.write_bytes(binary);output.chmod(0o755)
    return {'version':lock['version'],'sha256':hashlib.sha256(binary).hexdigest(),'archive_integrity':actual}

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    print(json.dumps(fetch(args.output)))
