"""Exercise context selection on disposable projects using a supplied binary."""
from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile

from onboarding import require, snapshot


def exercise_context(binary: Path, parent: Path, environment: dict) -> dict:
    calls, measurements = [], []
    with tempfile.TemporaryDirectory(prefix='context-selection-', dir=parent) as temporary:
        work = Path(temporary)

        def invoke(*args, expected=0):
            out = subprocess.run([str(binary), *args], cwd=work, env=environment,
                                 capture_output=True, text=True, timeout=60, shell=False)
            require(out.returncode == expected, 'context: unexpected exit; output omitted')
            value = json.loads(out.stderr if expected == 2 else out.stdout)
            calls.append({'args': list(args), 'exit': out.returncode})
            return value

        for variant in ('base', 'web-app'):
            for mode in ('minimal', 'full'):
                name = f'{variant}-{mode}'
                result = invoke('init', name, '--boilerplate', variant, '--context-profile', mode)
                require(result['context_profile'] == mode, 'context: wrong selected mode')
                target = work / name
                require(invoke('validate', name)['valid'] is True, 'context: invalid project')
                files = snapshot(target)
                startup = ['AGENTS.md', '.agentic/README.md', '.agentic/manifest.yaml']
                measurements.append({'variant': variant, 'context_profile': mode, 'files': len(files),
                                     'utf8_bytes': sum((target / p).stat().st_size for p in files),
                                     'startup_route_utf8_bytes': sum((target / p).stat().st_size for p in startup)})
                if mode == 'minimal':
                    require(not (target / '.agentic/REFERENCE.md').exists()
                            and not (target / '.agentic/docs').exists()
                            and not (target / '.github/copilot-instructions.md').exists(),
                            'context: unexpected optional scaffolding')
                    require((target / '.agentic/THIRD_PARTY_NOTICES.md').is_file(),
                            'context: missing attribution')
                result = invoke('upgrade', name)
                require(result['context_profile'] == mode and result['boilerplate'] == variant,
                        'context: upgrade lost selection')
                require(snapshot(target) == files, 'context: repeated upgrade changed bytes')

        target = work / 'web-app-minimal'
        (target / '.agentic/PRODUCT.md').write_text('Synthetic accepted product truth\n')
        (target / '.agentic/REFERENCE.md').write_text('Synthetic authored optional context\n')
        before = snapshot(target)
        for mode in ('full', 'minimal'):
            result = invoke('upgrade', 'web-app-minimal', '--context-profile', mode)
            require(result['removed'] == [], 'context: mode change removed files')
            after = snapshot(target)
            require(all(after.get(p) == digest for p, digest in before.items()
                        if p not in ('.agentic/manifest.yaml', '.agentic/lock.json')),
                    'context: mode change replaced authored bytes')
            require(invoke('validate', 'web-app-minimal')['valid'] is True,
                    'context: invalid expanded project')
            before = after
        invoke('init', 'invalid', '--context-profile', 'unknown', expected=2)
        require(not (work / 'invalid').exists(), 'context: invalid selection wrote files')
    return {'passed': True, 'checks': calls, 'measurements': measurements,
            'scope': 'installed context, complete selected modules and startup route; no model outcome measurement'}
