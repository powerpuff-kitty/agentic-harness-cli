"""Preserve byte identity of the small adapter payload set at build time.

No runtime fetching, user-project changes, or rewriting of existing checkouts.
"""
from pathlib import Path
import subprocess

ADAPTER_FILES = (
    'LICENSE',
    'adapters/assets.json',
    'adapters/claude/files/CLAUDE.md',
    'adapters/claude/files/.claude/rules/agentic-typed-ui.md',
    'adapters/cursor/files/.cursor/rules/agentic-typed-ui.mdc',
)


def clone_source(url: str, target: Path) -> None:
    # Repository-local clone settings override host defaults before checkout.
    # Never change global Git configuration or rewrite an existing checkout.
    subprocess.run(['git', 'clone', '--quiet', '--no-checkout',
                    '--config', 'core.autocrlf=false', '--config', 'core.eol=lf',
                    url, str(target)], check=True)


def verify_adapter_bytes(repository: Path) -> None:
    for relative in ADAPTER_FILES:
        path = repository / relative
        if path.is_symlink() or not path.is_file():
            raise RuntimeError('sources: missing or linked adapter input')
        if path.stat().st_size > 262144:
            raise RuntimeError('sources: oversized adapter input')
        try:
            committed = subprocess.check_output(
                ['git', '-C', str(repository), 'show', 'HEAD:' + relative],
                stderr=subprocess.DEVNULL)
            working = path.read_bytes()
        except (OSError, subprocess.CalledProcessError):
            raise RuntimeError('sources: adapter byte verification failed') from None
        if working != committed:
            raise RuntimeError('sources: adapter checkout differs from pinned Git bytes; '
                               'preserve existing inputs and prepare a fresh checkout')
