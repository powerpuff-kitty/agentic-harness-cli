#!/usr/bin/env python3
"""Run actual caller-approved completion probes against the pinned schema."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from jsonschema import Draft202012Validator
from completion_probe import exercise_completion
root = Path(__file__).resolve().parents[1]
binary = Path(sys.argv[1]).resolve(strict=True)
schema = json.loads((root / 'upstream/agentic-harness/catalog/schema/check-completion.v1.schema.json').read_text())
validator = Draft202012Validator(schema)
with tempfile.TemporaryDirectory(prefix='ah-completion-') as directory:
    working = Path(directory)
    def run(*argv, exits=(0,), json_output=True):
        p = subprocess.run([str(binary), *argv], cwd=working, capture_output=True, text=True, timeout=120)
        assert p.returncode in exits, (argv[:2], p.returncode, p.stderr)
        return json.loads(p.stdout if p.returncode == 0 else p.stderr)
    report = exercise_completion(run, working, validator.validate)
    print(json.dumps(report, indent=2))
