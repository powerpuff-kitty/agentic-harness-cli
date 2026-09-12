"""Executable checks for the read-only planner; repository commands never execute."""
import hashlib
import json
from pathlib import Path


def framed(domain, fields):
    h = hashlib.sha256(domain)
    for field in fields:
        h.update(len(field).to_bytes(8, 'big'))
        h.update(field)
    return 'sha256:' + h.hexdigest()


def exercise_plan(run, root):
    target = root / 'review'
    (target / '.agentic').mkdir(parents=True)
    (target / 'src').mkdir()
    (target / 'src/main.ts').write_bytes(b'export const value = 1;\n')
    policy = {'format_version':1,'kind':'check-policy','inputs':['src'],
              'checks':[{'id':'unit','argv':['missing-synthetic-tool'],'cwd':'.','required':True,
                         'timeout_ms':60000,'max_output_bytes':65536}],
              'required_controls':[{'rule_id':'fixture.boundary','capability':'enforced'}],
              'max_age_ms':3600000}
    config = target / '.agentic/checks.json'
    config.write_text(json.dumps(policy), encoding='utf-8')
    before = {p.relative_to(target).as_posix(): p.read_bytes() for p in target.rglob('*') if p.is_file()}
    one = run('checks', 'plan', 'review')
    two = run('checks', 'plan', 'review')
    assert one == two
    assert one['kind'] == 'check-plan' and one['format_version'] == 1
    for key in ['checks_executed','execution_permitted','executable_identity_verified']:
        assert one[key] is False
    assert one['control_requirements'][0]['status'] == 'unverified'
    assert before == {p.relative_to(target).as_posix(): p.read_bytes() for p in target.rglob('*') if p.is_file()}
    assert one['policy_digest'] == 'sha256:' + hashlib.sha256(config.read_bytes()).hexdigest()
    fields = []
    for entry in one['inputs']:
        if entry['kind'] == 'file':
            assert entry['digest'] == 'sha256:' + hashlib.sha256((target / entry['path']).read_bytes()).hexdigest()
        fields.extend([entry['path'].encode(), entry['kind'].encode(), (entry['digest'] or '').encode()])
    assert one['source_digest'] == framed(b'ah-check-inputs-v1\0', fields)
    assert one['review_digest'] == framed(b'ah-check-plan-v1\0', [one[k].encode() for k in ['policy_digest','source_digest','planner_digest']])
    (target / 'src/main.ts').write_bytes(b'export const value = 2;\n')
    changed = run('checks', 'plan', 'review')
    assert one['review_digest'] != changed['review_digest']
    for args in [('checks','run','review'), ('checks','plan','review','--apply')]:
        run(*args, exits=(2,), json_output=False)
    return {'passed':True,'kind':'read-only-planning-verification',
            'checks_executed':False,'unresolved_executable_accepted_only_as_preview':True,
            'input_change_detected':True,'hash_framing_verified':True}
