"""Authored synthetic evidence only; never auto-approves arbitrary project manifests."""
import copy
import hashlib
import json
import sys
import time
from pathlib import Path


def digest(data):
    return 'sha256:' + hashlib.sha256(data).hexdigest()


def exercise_completion(run, working, validate=lambda value: None):
    target = working / 'completion-fixture'
    (target / '.agentic').mkdir(parents=True)
    (target / 'src').mkdir()
    (target / 'evidence').mkdir()
    (target / 'src/main.txt').write_text('synthetic source')

    def save(path, value):
        data = json.dumps(value, sort_keys=True).encode()
        (target / path).write_bytes(data)
        return {'path': path, 'digest': digest(data)}

    policy = {'format_version': 1, 'kind': 'check-policy', 'inputs': ['src'],
              'checks': [{'id': 'unit', 'argv': ['python', '-c', "print('synthetic check')"], 'cwd': '.', 'required': True, 'timeout_ms': 30000, 'max_output_bytes': 4096}],
              'required_controls': [{'rule_id': 'synthetic-boundary', 'capability': 'checked'}], 'max_age_ms': 86400000}
    save('.agentic/checks.json', policy)
    settings = {'format_version': 1, 'kind': 'check-execution-settings', 'tools': {'python': str(Path(sys.executable).resolve())}, 'environment': {'PATH': ''}, 'max_total_ms': 300000}
    save('.agentic/check-execution.json', settings)
    review = run('checks', 'prepare', str(target))
    if not review['execution_supported']:
        return {'passed': True, 'execution_supported': False, 'completion_verified': False, 'cases': ['platform execution unavailable; positive completion not claimed']}
    result = run('checks', 'run', str(target), '--approve-review', review['approval_digest'], '--allow-unsandboxed')
    assert result['checks_passed'] and result['completion_verified'] is False
    producer = {'id': 'synthetic-probe', 'version': '1'}
    evidence = {'format_version': 1, 'kind': 'governance-evidence', 'policy_digest': review['plan']['policy_digest'], 'source_digest': review['plan']['source_digest'],
                'revision': None, 'producer': producer, 'observed_at_ms': int(time.time()*1000), 'host': None, 'adapter': None, 'scope': ['src'],
                'claims': [{'rule_id': 'synthetic-boundary', 'capability': 'checked', 'status': 'verified', 'mechanism': 'authored synthetic assertion', 'evidence_refs': ['synthetic-result']}], 'not_checked': ['real host enforcement']}
    cases = []

    def manifest(run_value=None, governance=None):
        report = save('evidence/run.json', result if run_value is None else run_value)
        control = save('evidence/governance.json', evidence if governance is None else governance)
        control['producer'] = producer
        reference = save('evidence/reference.json', {'synthetic': True})
        reference['name'] = 'synthetic-result'
        return {'format_version': 1, 'kind': 'check-evidence-manifest', 'run': report, 'governance': [control], 'references': [reference]}

    def complete(value, expected=0, approval=None):
        entry = save('evidence/manifest.json', value)
        validate(value)
        output = run('checks', 'complete', str(target), '--evidence', entry['path'], '--approve-evidence', approval or entry['digest'], exits=(expected,), json_output=expected == 0)
        if expected == 0:
            validate(output)
            assert output['completion_verified'] and output['scope'] == 'declared-checks-and-required-controls'
            assert output['producer_authenticated'] is False
        return output

    accepted = complete(manifest())
    cases.append('current caller-approved run and required governance accepted')
    complete(manifest(), 2, 'sha256:' + '0'*64)
    cases.append('unapproved manifest refused')
    for field, value in [('checks_passed', False), ('inputs_current', False), ('ended_at_ms', 9007199254740991), ('started_at_ms', 0), ('results', []), ('completion_verified', True)]:
        changed = copy.deepcopy(result)
        changed[field] = value
        complete(manifest(run_value=changed), 2)
        cases.append('run rejected: ' + field)
    for field, value in [('required', False), ('argv', ['python', '-c', 'different'])]:
        changed = copy.deepcopy(result)
        changed['results'][0][field] = value
        complete(manifest(run_value=changed), 2)
        cases.append('result rejected: ' + field)
    changed = copy.deepcopy(result)
    changed['results'][0]['outcome']['direct_child_reaped'] = False
    complete(manifest(run_value=changed), 2)
    cases.append('cleanup integrity failure rejected')
    for field, value in [('observed_at_ms', 0), ('scope', ['other']), ('claims', []), ('source_digest', 'sha256:'+'0'*64)]:
        changed = copy.deepcopy(evidence)
        changed[field] = value
        complete(manifest(governance=changed), 2)
        cases.append('governance rejected: ' + field)
    changed = copy.deepcopy(evidence)
    changed['claims'][0]['capability'] = 'declared'
    complete(manifest(governance=changed), 2)
    cases.append('declared cannot substitute for checked')
    changed = copy.deepcopy(evidence)
    changed['claims'].append(copy.deepcopy(changed['claims'][0]))
    complete(manifest(governance=changed), 2)
    cases.append('duplicate claims rejected')
    value = manifest()
    (target / 'evidence/reference.json').write_text('changed')
    complete(value, 2)
    cases.append('changed reference bytes rejected')
    value = manifest()
    value['references'][0]['path'] = '../outside'
    complete(value, 2)
    cases.append('path traversal rejected')
    value = manifest()
    (target / 'src/main.txt').write_text('changed after execution')
    complete(value, 2)
    (target / 'src/main.txt').write_text('synthetic source')
    cases.append('post-invocation source drift rejected')
    changed = dict(settings, environment={'PATH': '', 'SYNTHETIC': 'changed'})
    save('.agentic/check-execution.json', changed)
    complete(manifest(), 2)
    save('.agentic/check-execution.json', settings)
    cases.append('settings drift rejected')
    value = manifest()
    value['governance'] = []
    complete(value, 2)
    cases.append('missing required governance report rejected')
    value = manifest()
    value['governance'][0]['producer'] = {'id': 'different-producer', 'version': '1'}
    complete(value, 2)
    cases.append('producer binding mismatch rejected')
    value = manifest()
    run('checks', 'complete', str(target), '--evidence', 'evidence/manifest.json', exits=(2,), json_output=False)
    cases.append('missing caller approval rejected')
    if sys.platform != 'win32':
        value = manifest()
        link = target / 'evidence/reference-link.json'
        link.symlink_to(target / 'evidence/reference.json')
        value['references'][0]['path'] = 'evidence/reference-link.json'
        complete(value, 2)
        link.unlink()
        cases.append('symlink reference rejected')
    complete(manifest())
    return {'passed': True, 'execution_supported': True, 'cases': cases, 'accepted': accepted,
            'limitations': ['Synthetic caller trust only; no signed producer or native coding-host claim.', 'Scoped completion is not whole-project readiness.']}
