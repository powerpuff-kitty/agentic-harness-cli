#!/usr/bin/env python3
"""Package complete attribution bundles; verify integrity before executing binaries."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import zipfile


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def companions(binary):
    return [Path(str(binary) + suffix) for suffix in
            ('.sha256', '.provenance.json', '.notices.txt', '.notices.json', '.notices.rust.html')]


def verify(binary, execute=True, require_authored=False):
    checksum, provenance, notices, metadata, rust_notices = companions(binary)
    sha256 = digest(binary)
    require(checksum.read_text() == f'{sha256}  {binary.name}\n', 'checksum mismatch')
    evidence = json.loads(provenance.read_text())
    require(evidence['asset'] == binary.name and evidence['sha256'] == sha256, 'provenance mismatch')
    if evidence['format_version'] == 2:
        require(evidence['attribution'] == {p.name: digest(p) for p in (notices, metadata, rust_notices)}, 'attribution checksum mismatch')
        report = json.loads(metadata.read_text())
        require(report['notice_sha256'] == digest(notices), 'notice metadata mismatch')
        require(report['rust_library_sha256'] == digest(rust_notices), 'Rust notice metadata mismatch')
        require(report['cargo_lock_sha256'] == evidence['cargo_lock_sha256'], 'notice lockfile mismatch')
        require(report['toolchain'] == evidence['toolchain'], 'notice toolchain mismatch')
        require(report['embedded_sources'] == evidence['version']['sources'], 'notice source identity mismatch')
        if require_authored:
            require(not report['missing_authored_licenses'], 'authored-source licensing remains unresolved')
    else:
        require(evidence['format_version'] == 1 and not require_authored, 'attribution bundle required')
    # Verification must finish before even --version executes downloaded code.
    if execute:
        version = json.loads(subprocess.check_output([str(binary), '--version']))
        require(evidence['version'] == version, 'binary version mismatch')
    return evidence


def package(binary, commit, notices):
    checksum, provenance, text_target, metadata_target, rust_target = companions(binary)
    for source, target in [(notices, text_target), (notices.with_suffix('.json'), metadata_target),
                           (notices.with_suffix('.rust.html'), rust_target)]:
        if source.resolve() != target.resolve():
            shutil.copyfile(source, target)
    sha256 = digest(binary)
    version = json.loads(subprocess.check_output([str(binary), '--version']))
    checksum.write_text(f'{sha256}  {binary.name}\n', encoding='utf-8')
    evidence = {'format_version': 2, 'asset': binary.name, 'sha256': sha256,
                'commit': commit, 'version': version,
                'cargo_lock_sha256': digest(Path(__file__).resolve().parent.parent / 'Cargo.lock'),
                'toolchain': subprocess.check_output(['rustc', '--version'], text=True).strip(),
                'attribution': {p.name: digest(p) for p in (text_target, metadata_target, rust_target)}}
    provenance.write_text(json.dumps(evidence, indent=2) + '\n', encoding='utf-8')
    verify(binary, execute=False)
    archive = Path(str(binary) + '.zip')
    with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as bundle:
        for path in [binary, *companions(binary)]:
            info = zipfile.ZipInfo(path.name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (0o100755 if path == binary else 0o100644) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            bundle.writestr(info, path.read_bytes())
    Path(str(archive) + '.sha256').write_text(f'{digest(archive)}  {archive.name}\n', encoding='utf-8')


def verify_archive(archive, require_authored=False):
    require(archive.suffix == '.zip', 'expected candidate zip')
    require(Path(str(archive) + '.sha256').read_text() == f'{digest(archive)}  {archive.name}\n', 'archive checksum mismatch')
    with tempfile.TemporaryDirectory(prefix='ah-bundle-verify-') as directory:
        binary = Path(directory) / archive.stem
        expected = {p.name for p in [binary, *companions(binary)]}
        with zipfile.ZipFile(archive) as bundle:
            require(len(bundle.namelist()) == len(expected) and set(bundle.namelist()) == expected, 'unexpected archive members')
            require(sum(i.file_size for i in bundle.infolist()) <= 256 * 1024 * 1024, 'bundle exceeds size limit')
            for name in sorted(expected):
                (Path(directory) / name).write_bytes(bundle.read(name))
        # Archive review supports all target platforms without executing their code.
        evidence = verify(binary, execute=False, require_authored=require_authored)
        require(evidence['format_version'] == 2, 'archive requires attribution provenance v2')
        return evidence


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('binary', type=Path)
    parser.add_argument('--commit')
    parser.add_argument('--notices', type=Path)
    parser.add_argument('--verify', action='store_true')
    parser.add_argument('--verify-archive', action='store_true')
    parser.add_argument('--require-authored-licenses', action='store_true')
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    if args.verify_archive:
        verify_archive(binary, args.require_authored_licenses)
    elif args.verify:
        verify(binary, require_authored=args.require_authored_licenses)
    else:
        if not (args.commit and args.notices):
            parser.error('--commit and --notices are required when packaging')
        package(binary, args.commit, args.notices)


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, KeyError, zipfile.BadZipFile) as error:
        print(f'Candidate verification failed: {error}', file=sys.stderr)
        raise SystemExit(1)
