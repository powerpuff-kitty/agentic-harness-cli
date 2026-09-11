#!/usr/bin/env python3
"""Repeatable CLI latency/RSS benchmark. Output is evidence, not a universal SLA."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument('binary', type=Path)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--sizes', type=int, nargs='+', default=[100, 1000, 5000])
parser.add_argument('--runs', type=int, default=5)
parser.add_argument('--baseline', type=Path, help='Compare with a previous result from the same environment')
parser.add_argument('--target', type=Path, help='Optional read-only representative repository')
args = parser.parse_args()
assert args.runs >= 3 and all(n > 0 for n in args.sizes)
binary = args.binary.resolve(strict=True)
rows = []
environment = os.environ.copy()
environment.pop('AH_REGISTRY', None)

def measure(root, command, workload, size):
    warmup = subprocess.run([str(binary), *command, str(root)], stdout=subprocess.PIPE,
                   stderr=subprocess.PIPE, env=environment, check=False)
    samples, rss, exits = [], [], []
    for _ in range(args.runs):
        start = time.perf_counter()
        child = subprocess.Popen([str(binary), *command, str(root)], stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL, env=environment)
        if hasattr(os, 'wait4'):
            _, status, usage = os.wait4(child.pid, 0)
            child.returncode = os.waitstatus_to_exitcode(status)
            rss.append(usage.ru_maxrss / (1024 * 1024 if platform.system() == 'Darwin' else 1024))
        else:
            child.wait()
        samples.append(time.perf_counter() - start)
        exits.append(child.returncode)
        assert child.returncode in (0, 1), (command, child.returncode)
    rows.append({'workload': workload, 'size': size, 'command': list(command),
                 'seconds': samples, 'median_seconds': statistics.median(samples),
                 'min_seconds': min(samples), 'max_seconds': max(samples),
                 'stdev_seconds': statistics.stdev(samples), 'peak_rss_mib': rss,
                 'exit_codes': exits, 'output_bytes': len(warmup.stdout)})

with tempfile.TemporaryDirectory(prefix='ah-benchmark-') as directory:
    for size in args.sizes:
        root = Path(directory) / str(size)
        (root / 'src').mkdir(parents=True)
        (root / 'package.json').write_text('{"dependencies":{"vue":"3"}}')
        for i in range(size):
            link = f'import C from "./C{i - 1}.vue";\n' if i else ''
            (root / 'src' / f'C{i}.vue').write_text(
                '<script setup lang="ts">\n' + link + 'const value = 1;\n</script>\n'
                '<template><button>Save</button></template>\n<style>.button {color: #336699; padding: 8px;}</style>\n')
        for command in [('audit',), ('architecture', 'analyze'), ('design-system-components',)]:
            measure(root, command, 'vue-chain', size)
    for count in [10, 100, 500]:
        root = Path(directory) / f'skills-{count}'
        for i in range(count):
            path = root / '.agents' / 'skills' / f'skill-{i}' / 'SKILL.md'
            path.parent.mkdir(parents=True)
            path.write_text('# Skill\nRead relevant context. Verify tests before completion.\n' * 10)
        measure(root, ('agentic', 'skills'), 'identical-skills', count)
    if args.target:
        root = args.target.resolve(strict=True)
        for command in [('audit',), ('architecture', 'analyze'), ('design-system-components',)]:
            measure(root, command, 'representative-repository', None)
report = {'format_version': 1, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
          'version': json.loads(subprocess.check_output([str(binary), '--version'])),
          'environment': {'platform': platform.platform(), 'machine': platform.machine(),
                          'processor': platform.processor(), 'python': platform.python_version()},
          'method': 'One warmup then repeated fresh processes; stdout discarded; per-child peak RSS via wait4 where available.',
          'results': rows}
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(report, indent=2) + '\n')
for row in rows:
    print(row['workload'], row['size'], ' '.join(row['command']), round(row['median_seconds'], 4), 's')

if args.baseline:
    baseline = json.loads(args.baseline.read_text())
    assert baseline['environment'] == report['environment'], 'Refuse cross-environment timing comparison'
    lookup = {(r['workload'], r['size'], tuple(r['command'])): r for r in baseline['results']}
    failures = []
    for row in rows:
        prior = lookup[(row['workload'], row['size'], tuple(row['command']))]
        allowance = max(0.005, prior['median_seconds'] * 0.15, 3 * (prior['stdev_seconds'] + row['stdev_seconds']))
        if row['median_seconds'] > prior['median_seconds'] + allowance:
            failures.append({'command':row['command'], 'size':row['size'], 'limit_seconds':prior['median_seconds'] + allowance, 'actual_seconds':row['median_seconds']})
    assert not failures, json.dumps(failures)
