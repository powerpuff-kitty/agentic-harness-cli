#!/usr/bin/env python3
"""Check real CLI output against pinned canonical public schemas."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
from jsonschema import Draft202012Validator
p = argparse.ArgumentParser()
p.add_argument('binary', type=Path)
a = p.parse_args()
binary = a.binary.resolve(strict=True)
root = Path(__file__).resolve().parents[1]
schemas = root / 'upstream/agentic-harness/catalog/schema'
with tempfile.TemporaryDirectory(prefix='ah-contracts-') as directory:
    target = Path(directory)
    (target / 'src').mkdir()
    (target / 'src/main.ts').write_text("import './missing';\n")
    for args, schema in [(['audit'], 'codebase-audit.v2.schema.json'),
                         (['agentic', 'audit'], 'agentic-readiness.v2.schema.json')]:
        output = subprocess.run([str(binary), *args, str(target)], capture_output=True, text=True, timeout=60)
        assert output.returncode in (0, 1), output.stderr
        validator = Draft202012Validator(json.loads((schemas / schema).read_text()))
        validator.validate(json.loads(output.stdout))
print('Actual CLI outputs conform to pinned canonical v2 schemas')
