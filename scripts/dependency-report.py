#!/usr/bin/env python3
"""Record exact resolved package licenses; do not infer an undeclared project license."""
import argparse
import json
from pathlib import Path
import subprocess
p = argparse.ArgumentParser()
p.add_argument('--output', type=Path, required=True)
a = p.parse_args()
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1']))
packages = [{'name': p['name'], 'version': p['version'], 'license': p['license'], 'source': p['source'],
             'repository': p['repository']} for p in metadata['packages']]
report = {'format_version': 1, 'packages': sorted(packages, key=lambda p: (p['name'], p['version'])),
          'missing_license': [p['name'] for p in packages if not p['license']],
          'embedded_sources': json.loads(Path('upstream.lock.json').read_text()),
          'note': 'SPDX metadata inventory; verify embedded-source licenses and retain required notices before publishing.'}
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text(json.dumps(report, indent=2) + '\n')
print(f"{len(packages)} packages inventoried; missing license: {report['missing_license']}")
