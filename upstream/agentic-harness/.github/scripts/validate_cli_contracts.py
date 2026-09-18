#!/usr/bin/env python3
"""Validate public CLI contracts and positive/negative fixtures."""
import copy
import json
from pathlib import Path
import subprocess
import sys
from jsonschema import Draft202012Validator
root = Path(__file__).resolve().parents[2]
for path in (root / 'catalog/schema').glob('*.schema.json'):
    Draft202012Validator.check_schema(json.loads(path.read_text()))
schema = json.loads((root / 'catalog/schema/codebase-audit.v2.schema.json').read_text())
validator = Draft202012Validator(schema)
fixture = json.loads((root / '.agentic/evals/fixtures/codebase-audit.v2.json').read_text())
validator.validate(fixture)
for key, value in [('overall', 101), ('scores', {}), ('scores', {'testing': 'good'}),
                   ('format_version', 3), ('findings', [{}]), ('checks', {}),
                   ('architecture', {'compliance': {'deterministic_errors': -1, 'passed': True}})]:
    invalid = copy.deepcopy(fixture)
    invalid[key] = value
    assert not validator.is_valid(invalid), (key, value)
assert not validator.is_valid({})
print('CLI schemas and audit compatibility fixtures passed')

for name, fixture, invalid_field in [
    ('audit-gate.v1.schema.json', {'format_version': 1, 'kind': 'audit-gate', 'passed': False, 'failures': ['incomplete evidence']}, 'passed'),
    ('audit-comparison.v1.schema.json', {'format_version': 1, 'kind': 'audit-comparison', 'overall': {'before': None, 'after': 80, 'delta': None}, 'scores': {'testing': {'before': 50, 'after': 80, 'delta': 30}}}, 'overall'),
]:
    validator = Draft202012Validator(json.loads((root / 'catalog/schema' / name).read_text()))
    validator.validate(fixture)
    invalid = copy.deepcopy(fixture)
    invalid[invalid_field] = 'invalid'
    assert not validator.is_valid(invalid), name
    invalid = copy.deepcopy(fixture)
    del invalid['kind']
    assert not validator.is_valid(invalid), name
print('Gate and comparison compatibility fixtures passed')

for script in ['validate_check_contracts.py', 'validate_execution_contracts.py',
               'validate_decision_contracts.py',
               'validate_source_graph_contract.py', 'validate_adapter_contracts.py',
               'test_public_archive_names.py', 'test_public_surface.py']:
    subprocess.run([sys.executable, str(root / '.github/scripts' / script)], check=True)
