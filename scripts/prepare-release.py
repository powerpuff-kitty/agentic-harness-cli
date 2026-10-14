#!/usr/bin/env python3
"""Prepare reviewed platform bundles and notes from a successful exact-commit CI run.

Reads GitHub with gh; writes a new local output directory. No tags, release,
publication, deployment, or target-platform binary execution.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import zipfile

ROOT=Path(__file__).resolve().parents[1]
REPOSITORY='powerpuff-kitty/agentic-harness-cli'
ASSETS=['ah-linux-x86_64','ah-macos-x86_64','ah-macos-arm64','ah-windows-x86_64.exe']

def gh(*args):return subprocess.check_output(['gh',*args],text=True)

def prepare(run_id,output):
 if not run_id.isdigit():raise ValueError('CI run ID must be numeric')
 if output.exists():raise ValueError('Choose a new preparation directory')
 commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
 if subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip():raise ValueError('Release preparation requires a clean source checkout')
 run=json.loads(gh('api',f'repos/{REPOSITORY}/actions/runs/{run_id}'))
 if run['status']!='completed' or run['conclusion']!='success' or run['head_sha']!=commit:raise ValueError('CI must pass for the exact checked-out commit')
 if run['path']!='.github/workflows/ci.yml' or run['head_repository']['full_name']!=REPOSITORY:raise ValueError('Unexpected CI workflow/repository')
 jobs=json.loads(gh('api',f'repos/{REPOSITORY}/actions/runs/{run_id}/jobs?per_page=100'))['jobs']
 required={'dependencies','native-skills'}|{f'{kind} ({os_name}, {asset})' for kind in ['build','installed'] for os_name,asset in [('ubuntu-latest','ah-linux-x86_64'),('macos-15-intel','ah-macos-x86_64'),('macos-latest','ah-macos-arm64'),('windows-latest','ah-windows-x86_64.exe')]}
 completed={job['name'] for job in jobs if job['conclusion']=='success'}
 if not required<=completed:raise ValueError('Required CI jobs missing or not successful: '+', '.join(sorted(required-completed)))
 version=tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version']
 with tempfile.TemporaryDirectory(prefix='ah-release-prepare-') as raw:
  stage=Path(raw);bundles={}
  for asset in ASSETS:
   folder=stage/asset;folder.mkdir();gh('run','download',run_id,'--repo',REPOSITORY,'--name',asset,'--dir',str(folder))
   archive=folder/(asset+'.zip');sidecar=folder/(asset+'.zip.sha256')
   digest=hashlib.sha256(archive.read_bytes()).hexdigest()
   if sidecar.read_text().split()!=[digest,archive.name]:raise ValueError('Archive checksum mismatch: '+asset)
   subprocess.run(['python3',str(ROOT/'scripts/package-candidate.py'),str(archive),'--verify-archive','--require-authored-licenses'],check=True,capture_output=True,text=True)
   with zipfile.ZipFile(archive) as bundle:
    provenance=json.loads(bundle.read(asset+'.provenance.json'))
   if provenance['commit']!=commit or provenance['version']['version']!=version:raise ValueError('Bundle commit/version mismatch: '+asset)
   if provenance['version']['sources']!=json.loads((ROOT/'upstream.lock.json').read_text()):raise ValueError('Bundle source pins mismatch: '+asset)
   bundles[archive.name]={'sha256':digest,'bytes':archive.stat().st_size,'path':archive,'sidecar':sidecar}
  evidence=stage/'evidence';evidence.mkdir()
  for artifact in ['dependency-evidence','native-skill-evidence']:
   gh('run','download',run_id,'--repo',REPOSITORY,'--name',artifact,'--dir',str(evidence))
  evidence_hashes={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in evidence.iterdir() if p.is_file()}
  output.mkdir(parents=True)
  shutil.copytree(evidence,output/'evidence')
  for entry in bundles.values():
   shutil.copy2(entry.pop('path'),output);shutil.copy2(entry.pop('sidecar'),output)
  (output/'SHA256SUMS').write_text(''.join(f"{entry['sha256']}  {name}\n" for name,entry in sorted(bundles.items())))
  notes=(ROOT/'CHANGELOG.md').read_text().split('## '+version,1)[1].split('\n## ',1)[0].split('\n',1)[1].strip()
  (output/'RELEASE_NOTES.md').write_text(f'# Agentic Harness CLI {version}\n\nSource: {commit}\nCI: {run["html_url"]}\n\n'+notes+'\n\nReview docs/operations.md for bundle verification, installation and recovery.\nPreparation does not publish this candidate.\n')
  result={'format_version':1,'version':version,'proposed_tag':'v'+version,'commit':commit,'ci_run':run_id,'ci_url':run['html_url'],'required_jobs':sorted(required),'evidence_sha256':evidence_hashes,'bundles':bundles,'publication':'not_requested_or_performed','limitations':['checksums are not signatures','CI/native loading are not general model-quality evidence','stable publication requires an explicit release action']}
  (output/'candidate.json').write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--run',required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();print(json.dumps(prepare(a.run,a.output.resolve()),indent=2))
