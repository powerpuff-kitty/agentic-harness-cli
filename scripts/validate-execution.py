#!/usr/bin/env python3
"""Validate actual synthetic review/run output with locally registered pinned schemas."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from jsonschema import Draft202012Validator
from referencing import Registry, Resource
from check_execution_probe import exercise_execution

root = Path(__file__).resolve().parents[1]
binary = Path(sys.argv[1]).resolve(strict=True)
schemas = root / 'upstream/agentic-harness/catalog/schema'
core = json.loads((schemas / 'checks.v1.schema.json').read_text())
execution = json.loads((schemas / 'check-execution.v1.schema.json').read_text())
registry = Registry().with_resource(core['$id'], Resource.from_contents(core))
validator = Draft202012Validator(execution, registry=registry)
with tempfile.TemporaryDirectory(prefix='ah-execution-contracts-') as directory:
    working = Path(directory)
    def run(*argv, exits=(0,), json_output=True):
        process = subprocess.run([str(binary), *argv], cwd=working, capture_output=True, text=True, timeout=60)
        assert process.returncode in exits, (argv, process.returncode, process.stdout, process.stderr)
        return json.loads(process.stdout) if json_output else process.stdout
    evidence = exercise_execution(run, working, validator.validate)
    assert evidence['passed'] is True
print('Actual synthetic review and execution outputs match pinned offline-resolved schemas')
