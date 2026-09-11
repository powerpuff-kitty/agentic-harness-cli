#!/usr/bin/env python3
"""Collect pinned dependency attribution without choosing a license for this project."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def digest(data):
    return hashlib.sha256(data).hexdigest()


def notice_files(root):
    return sorted(p for p in root.rglob('*') if p.is_file() and not p.is_symlink()
                  and (p.name.lower().startswith(('license', 'licence', 'copying', 'copyright',
                                                  'notice', 'authors'))
                       or p.name.lower().endswith('.license')
                       or any(part.lower() in ('licenses', 'licences') for part in p.relative_to(root).parts)))


def collect(packages, sources, supplements):
    records, sections = [], []
    for package in sorted(packages, key=lambda p: (p['name'], p['version'])):
        if package['source'] is None:
            continue  # Authored sources are tracked separately, never assigned an inferred license.
        name, version = package['name'], package['version']
        if not package['license']:
            raise ValueError(f'{name} {version}: missing license metadata')
        root = Path(package['manifest_path']).parent
        files = [(str(p.relative_to(root)).replace('\\', '/'), p.read_bytes(), 'crate archive')
                 for p in notice_files(root)]
        if package.get('license_file'):
            license_file = Path(package['license_file'])
            license_file = license_file if license_file.is_absolute() else root / license_file
            if not license_file.resolve().is_relative_to(root.resolve()):
                raise ValueError(f'{name}: license file escapes crate')
            label = license_file.relative_to(root).as_posix()
            if label not in [f[0] for f in files]:
                files.append((label, license_file.read_bytes(), 'crate archive'))
        for source in sources:
            if {'name': name, 'version': version} not in source['packages']:
                continue
            vcs = json.loads((root / '.cargo_vcs_info.json').read_text())
            if vcs['git']['sha1'] != source['commit'] or package['repository'] != source['repository']:
                raise ValueError(f'{name}: supplemental license source does not match pinned crate')
            path = supplements / source['file']
            if not path.resolve().is_relative_to(supplements.resolve()):
                raise ValueError('supplemental license path escapes source directory')
            data = path.read_bytes()
            if digest(data) != source['sha256']:
                raise ValueError(f'{name}: supplemental license checksum mismatch')
            files.append((source['file'], data, source['source']))
        if not files:
            raise ValueError(f'{name} {version}: no attribution text; add a pinned source supplement')
        entry = {'name': name, 'version': version, 'license': package['license'],
                 'source': package['source'], 'repository': package['repository'], 'files': []}
        sections.append(f"\n{'=' * 72}\n{name} {version}\nDeclared license: {package['license']}\n")
        for label, data, origin in sorted(files):
            text = data.decode('utf-8')
            entry['files'].append({'path': label, 'source': origin, 'sha256': digest(data)})
            sections.append(f'\n--- {label} ({origin}) ---\n{text}\n')
        records.append(entry)
    return records, sections


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True, help='Notice text; metadata uses .json suffix')
    parser.add_argument('--require-authored-licenses', action='store_true')
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--locked', '--format-version', '1'], cwd=ROOT))
    sources = json.loads((ROOT / 'third-party/license-sources.json').read_text())['sources']
    records, sections = collect(metadata['packages'], sources, ROOT / 'third-party')
    authored = []
    for name, root in [('CLI', ROOT), ('canonical', ROOT / 'upstream/agentic-harness'),
                       ('agents', ROOT / 'upstream/agentic-harness-agents'),
                       ('model_registry', ROOT / 'upstream/agentic-harness-registry')]:
        # Only repository-root declarations apply to authored sources; do not mistake
        # a vendored dependency or template's LICENSE for the repository's declaration.
        files = sorted(p for p in root.glob('*') if p.is_file() and not p.is_symlink()
                       and p.name.lower().startswith(('license', 'licence', 'copying', 'notice')))
        licenses = [p for p in files if p.name.lower().startswith(('license', 'licence', 'copying'))]
        authored.append({'name': name, 'license_present': bool(licenses),
                         'files': [{'path': p.name, 'sha256': digest(p.read_bytes())} for p in files]})
        for path in files:
            sections.append(f'\n--- Authored source {name}: {path.name} ---\n{path.read_text(encoding="utf-8")}\n')
    missing = [source['name'] for source in authored if not source['license_present']]
    if any(p['source'] is None and not p['license'] and not p.get('license_file') for p in metadata['packages']):
        missing.append('CLI package metadata')
    header = ('Third-party attribution for Agentic Harness CLI\n'
              'Scope: all resolved Cargo packages, including build and target-specific dependencies.\n'
              'Original texts and declared license alternatives are retained. Authored-source terms\n'
              'appear in the source sections below; this collection does not certify legal compatibility.\n'
              'Rust standard-library attribution accompanies this file as .rust.html.\n'
              f'Authored sources without a root license declaration: {", ".join(missing) or "none"}.\n')
    toolchain = subprocess.check_output(['rustc', '--version'], text=True).strip()
    sysroot = Path(subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip())
    rust_notices = (sysroot / 'share/doc/rust/COPYRIGHT-library.html').read_bytes()
    data = (header + ''.join(sections)).encode('utf-8')
    report = {'format_version': 1, 'cargo_lock_sha256': digest((ROOT / 'Cargo.lock').read_bytes()),
              'notice_sha256': digest(data), 'packages': records,
              'rust_library_sha256': digest(rust_notices), 'toolchain': toolchain, 'authored_sources': authored,
              'missing_authored_licenses': missing,
              'embedded_sources': json.loads((ROOT / 'upstream.lock.json').read_text())}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
    args.output.with_suffix('.rust.html').write_bytes(rust_notices)
    args.output.with_suffix('.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(f'{len(records)} dependency attributions; undeclared authored licenses: {missing}')
    if args.require_authored_licenses and missing:
        raise SystemExit('Cannot prepare a release with undeclared authored-source licenses')


if __name__ == '__main__':
    main()
