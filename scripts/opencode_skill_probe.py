#!/usr/bin/env python3
"""Verify native V2 skill registration/content in a disposable persistent host.

Never invokes a model or changes an existing host installation. Requires an
explicit OpenCode executable, Git and Python standard library only.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request
import urllib.parse


def probe(executable, source, agents_export=None):
    source=Path(source).resolve()
    folders=sorted(p.parent for p in source.glob('*/SKILL.md'))
    if not folders:raise ValueError('No skill folders')
    with tempfile.TemporaryDirectory(prefix='ah-native-skills-') as raw:
        root=Path(raw).resolve();project=root/'project';project.mkdir()
        subprocess.run(['git','init','-q',str(project)],check=True)
        expected={}
        for folder in folders:
            if folder.is_symlink() or any(p.is_symlink() for p in folder.rglob('*')):raise ValueError('Linked skill input')
            target=project/'.opencode/skills'/folder.name
            shutil.copytree(folder,target)
            text=(target/'SKILL.md').read_text()
            if not text.startswith('---\n'):raise ValueError('Missing frontmatter')
            body=text.split('\n---\n',1)[1].strip()
            expected[folder.name]={'body':body,'path':str(target/'SKILL.md'),'sha256':hashlib.sha256(body.encode()).hexdigest()}
        expected_agents={};expected_commands={}
        if agents_export:
            export=Path(agents_export).resolve()
            for kind in ['agents','commands']:
                for file in sorted((export/kind).glob('*.md')):
                    target=project/'.opencode'/kind/file.name;target.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(file,target)
                    if kind=='agents':expected_agents[file.stem]=file.read_text().split('\n---\n',1)[1].strip()
                    else:expected_commands[file.stem]=True
        env={k:v for k,v in os.environ.items() if k in ['PATH','HOME','USER','LOGNAME','LANG','LC_ALL','SYSTEMROOT','WINDIR','TMPDIR']}
        for key,sub in [('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state')]:env[key]=str(root/sub)
        env['OPENCODE_CONFIG_CONTENT']='{"autoupdate":false}'
        env['OPENCODE_PASSWORD']=secrets.token_hex(24);env['OPENCODE_USERNAME']='opencode'
        with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
        server=subprocess.Popen([executable,'serve','--hostname','127.0.0.1','--port',str(port)],cwd=project,env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        url=f'http://127.0.0.1:{port}'
        try:
            auth=base64.b64encode(('opencode:'+env['OPENCODE_PASSWORD']).encode()).decode()
            for attempt in range(50):
                try:
                    request=urllib.request.Request(url+'/openapi.json',headers={'Authorization':'Basic '+auth})
                    with urllib.request.urlopen(request,timeout=1) as response:api=json.load(response)
                    break
                except (OSError,ValueError):
                    if server.poll() is not None:raise ValueError('Isolated server exited')
                    time.sleep(.1)
            else:raise ValueError('Isolated server readiness timed out')
            def call(operation):
                paths=[path for path,methods in api['paths'].items() if methods.get('get',{}).get('operationId')==operation]
                if len(paths)!=1:raise ValueError('Missing or ambiguous API operation: '+operation)
                query=urllib.parse.urlencode({'location[directory]':str(project)})
                request=urllib.request.Request(url+paths[0]+'?'+query,headers={'Authorization':'Basic '+auth})
                with urllib.request.urlopen(request,timeout=20) as response:raw=response.read(16_000_001)
                if len(raw)>16_000_000:raise ValueError('Host response exceeds probe bound')
                result=json.loads(raw)
                if result.get('location',{}).get('directory')!=str(project):raise ValueError('Host returned another location')
                return result['data']
            # Configuration plugins initialize asynchronously. Repeat on the SAME
            # server; a new --standalone server on each attempt repeats the race.
            for attempt in range(30):
                loaded={x['id']:x for x in call('skill.list')}
                if set(expected)<=loaded.keys():break
                time.sleep(.2)
            else:raise ValueError('Native skill IDs missing: '+', '.join(sorted(set(expected)-loaded.keys())))
            for name,entry in expected.items():
                actual=loaded[name]
                if actual.get('path')!=entry['path'] or actual.get('content','').strip()!=entry['body']:
                    raise ValueError('Native skill path/content drift: '+name)
            host_agents='not_checked';host_commands='not_checked'
            if agents_export:
                operations={v['operationId'] for path in api['paths'].values() for v in path.values() if isinstance(v,dict) and 'operationId' in v}
                if 'agent.list' not in operations:raise ValueError('agent.list unavailable in selected host')
                for _ in range(30):
                    loaded_agents={x['id']:x for x in call('agent.list')}
                    loaded_commands={x['name']:x for x in call('command.list')}
                    if set(expected_agents)<=loaded_agents.keys() and set(expected_commands)<=loaded_commands.keys():break
                    time.sleep(.2)
                for name,body in expected_agents.items():
                    if loaded_agents.get(name,{}).get('system','').strip()!=body:raise ValueError('Loaded agent drift: '+name)
                if not set(expected_commands)<=loaded_commands.keys():raise ValueError('Missing commands')
                host_agents=len(expected_agents);host_commands=len(expected_commands)
            return {'status':'passed','host_version':subprocess.check_output([executable,'--version'],text=True,env=env).strip(),'skills':len(expected),'attempts':attempt+1,'skill_body_sha256':{n:e['sha256'] for n,e in expected.items()},'agent_prompts':host_agents,'team_commands':host_commands,'model_execution':'not_run','scope':'temporary project, persistent private server, no existing installation/configuration writes'}
        finally:
            server.terminate()
            try:server.wait(timeout=5)
            except subprocess.TimeoutExpired:server.kill();server.wait()

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--opencode',required=True)
    parser.add_argument('--source',type=Path,default=Path(__file__).resolve().parents[1]/'upstream/skills')
    parser.add_argument('--agents-export',type=Path)
    args=parser.parse_args()
    print(json.dumps(probe(args.opencode,args.source,args.agents_export),indent=2))
