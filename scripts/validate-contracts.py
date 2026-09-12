#!/usr/bin/env python3
"""Check real CLI output against pinned canonical public schemas."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from jsonschema import Draft202012Validator
p = argparse.ArgumentParser()
p.add_argument('binary', type=Path)
a = p.parse_args()
binary = a.binary.resolve(strict=True)
root = Path(__file__).resolve().parents[1]
suite = unittest.defaultTestLoader.discover(str(root / 'scripts'), pattern='test_onboarding.py')
if not unittest.TextTestRunner(verbosity=1).run(suite).wasSuccessful():
    raise SystemExit('Onboarding runner regressions failed')
schemas = root / 'upstream/agentic-harness/catalog/schema'
with tempfile.TemporaryDirectory(prefix='ah-contracts-') as directory:
    target = Path(directory)
    (target / 'src').mkdir()
    (target / 'src/main.ts').write_text("import './missing';\n")
    (target / 'src/a.ts').write_text("import './b';\n")
    (target / 'src/b.ts').write_text("import './a';\n")
    for args, schema in [(['audit'], 'codebase-audit.v2.schema.json'),
                         (['agentic', 'audit'], 'agentic-readiness.v2.schema.json')]:
        output = subprocess.run([str(binary), *args, str(target)], capture_output=True, text=True, timeout=60)
        assert output.returncode in (0, 1), output.stderr
        validator = Draft202012Validator(json.loads((schemas / schema).read_text()))
        validator.validate(json.loads(output.stdout))
        if args == ['audit']:
            assert any(f['dimension'] == 'architecture' for f in json.loads(output.stdout)['findings'])
            (target / 'audit.json').write_text(output.stdout)
    for args, schema, expected_exit in [
        (['compare', str(target / 'audit.json'), str(target / 'audit.json')], 'audit-comparison.v1.schema.json', 0),
        (['gate', str(target / 'audit.json')], 'audit-gate.v1.schema.json', 1),
    ]:
        output = subprocess.run([str(binary), *args], capture_output=True, text=True, timeout=60)
        assert output.returncode == expected_exit, output.stderr
        Draft202012Validator(json.loads((schemas / schema).read_text())).validate(json.loads(output.stdout))
    policy = {'format_version':1,'kind':'check-policy','inputs':['src'],
              'checks':[{'id':'unit','argv':['missing-synthetic-tool'],'cwd':'.','required':True,
                         'timeout_ms':60000,'max_output_bytes':65536}],
              'required_controls':[{'rule_id':'fixture.boundary','capability':'enforced'}],
              'max_age_ms':3600000}
    (target / '.agentic').mkdir()
    (target / '.agentic/checks.json').write_text(json.dumps(policy), encoding='utf-8')
    output = subprocess.run([str(binary), 'checks', 'plan', str(target)], capture_output=True, text=True, timeout=60)
    assert output.returncode == 0, output.stderr
    preview = json.loads(output.stdout)
    Draft202012Validator(json.loads((schemas / 'checks.v1.schema.json').read_text())).validate(preview)
    assert preview['execution_permitted'] is False
    from check_plan_probe import framed
    planner_inputs = [(root / p).read_bytes() for p in ['src/checks.rs','src/check_inputs.rs','upstream.lock.json']]
    assert preview['planner_digest'] == framed(b'ah-check-planner-v1\0', planner_inputs)
print('Actual CLI outputs conform to pinned audit, agentic, gate, comparison and check-plan schemas')
