# Security

Report security issues privately through the repository's supported GitHub security-reporting channel when available.

The CLI may inspect repositories containing sensitive material. Secret-scan output must report location/type without printing secret values. Treat fetched canonical/agent source repositories as trusted only at the pinned revisions recorded in `upstream.lock.json`.

The built-in secret scanner is a high-signal baseline and is not a replacement for platform secret scanning, dependency vulnerability scanning, SAST, or runtime security testing.
