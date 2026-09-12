"""Only disposable synthetic programs are executed; never load a user's project policy."""
import json
import sys


def exercise_execution(run, root, validate=None):
    results = []
    for name, code, expected in [
        ('pass', "print('SYNTHETIC_LOG_MUST_NOT_BE_PUBLISHED')", 'passed'),
        ('fail', 'raise SystemExit(7)', 'failed'),
        ('timeout', 'import time; time.sleep(10)', 'timeout'),
        ('output-limit', "print('x' * 100000)", 'output-limit'),
        ('mutation', "from pathlib import Path; Path('src/main.txt').write_text('changed')", 'passed'),
    ]:
        relative = 'execution-' + name
        target = root / relative
        (target / '.agentic').mkdir(parents=True)
        (target / 'src').mkdir()
        source = target / 'src/main.txt'
        source.write_text('original', encoding='utf-8')
        policy = {'format_version': 1, 'kind': 'check-policy', 'inputs': ['src'],
                  'checks': [{'id': 'synthetic', 'argv': ['python', '-c', code], 'cwd': '.',
                              'required': True, 'timeout_ms': 50 if name == 'timeout' else 2000,
                              'max_output_bytes': 128 if name == 'output-limit' else 65536}],
                  'required_controls': [{'rule_id': 'synthetic.rule', 'capability': 'enforced'}],
                  'max_age_ms': 3600000}
        settings = {'format_version': 1, 'kind': 'check-execution-settings',
                    'tools': {'python': sys.executable}, 'environment': {'PATH': ''},
                    'max_total_ms': 10000}
        (target / '.agentic/checks.json').write_text(json.dumps(policy), encoding='utf-8')
        (target / '.agentic/check-execution.json').write_text(json.dumps(settings), encoding='utf-8')
        review = run('checks', 'prepare', relative)
        if validate:
            validate(review)
        assert review['checks_executed'] is False and review['execution_permitted'] is False
        assert review['inherit_environment'] is False
        supported = review['execution_supported']
        approval = review['approval_digest']
        expected_exit = 0 if supported and name == 'pass' else 1
        report = run('checks', 'run', relative, '--approve-review', approval,
                     '--allow-unsandboxed', exits=(expected_exit,))
        if validate:
            validate(report)
        assert report['completion_verified'] is False
        assert report['checks_executed'] is supported
        assert report['governance_controls'][0]['status'] == 'unverified'
        assert report['results'][0]['outcome']['status'] == (expected if supported else 'unsupported')
        assert report['checks_passed'] is (supported and name == 'pass')
        # The literal appears in reviewed argv, but raw stream content is not retained.
        outcome = report['results'][0]['outcome']
        assert outcome['output_disclosure'] == 'omitted'
        assert 'SYNTHETIC_LOG_MUST_NOT_BE_PUBLISHED' not in json.dumps(outcome)
        if supported:
            assert outcome['direct_child_reaped'] is True
        if name == 'mutation' and supported:
            assert report['inputs_current'] is False
        if name == 'pass':
            run('checks', 'run', relative, '--approve-review', approval, exits=(2,), json_output=False)
            source.write_text('changed after approval', encoding='utf-8')
            run('checks', 'run', relative, '--approve-review', approval,
                '--allow-unsandboxed', exits=(2,), json_output=False)
        results.append({'case': name, 'status': outcome['status'], 'spawned': outcome['spawned']})
    return {'passed': True, 'kind': 'local-execution-verification',
            'execution_supported': supported, 'cases': results,
            'limitations': ['Synthetic commands only; not hostile-code sandbox testing.',
                            'Windows execution is unsupported; refusal is tested instead.']}
