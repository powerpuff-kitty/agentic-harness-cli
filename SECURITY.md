# Security

Report security issues privately through the repository's supported GitHub security-reporting channel when available.

The CLI may inspect repositories containing sensitive material. Secret-scan output must report location/type without printing secret values. Treat fetched canonical/agent source repositories as trusted only at the pinned revisions recorded in `upstream.lock.json`.

The built-in secret scanner is a high-signal baseline and is not a replacement for platform secret scanning, dependency vulnerability scanning, SAST, or runtime security testing.

Scanning follows repository-local ignores, skips symlink entries, and excludes generated/vendor directories. Explicit context reads must stay inside the chosen target and are size bounded. Skipped/unsupported content is outside coverage; findings never establish the absence of other secret types or vulnerabilities.

Composition stages writes and preserves existing content, but cannot guarantee a transaction across a system crash. Backups and retained provenance support recovery. Source synchronization refuses modified upstream checkouts. Release candidates use locked dependencies, pinned workflow actions, downloaded-artifact checks, SHA-256 sidecars, and source/toolchain provenance. See [release gates](docs/release-checklist.md).
