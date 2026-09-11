# Production-core release gates

The package remains beta (`0.1.0`). A passing numeric audit score is not release approval. The production-core roadmap is tracked in [#51](https://github.com/powerpuff-kitty/agentic-harness-cli/issues/51).

A candidate needs evidence tied to its commit and binary checksums:

- Formatting, lint, unit/integration and public-schema checks pass, including invalid input, incomplete coverage, parser regressions, expiry and rollback injection.
- Every supported catalog selection generates valid current-layout projects; legacy and customized projects have explicit preservation/migration behavior.
- Linux x86_64, macOS x86_64/arm64 and Windows x86_64 downloaded artifacts pass outside-checkout tests with embedded registry/content. Advertise only platforms whose candidate checks pass.
- Repeated time/RSS/output-size benchmarks retain environment, warmup, samples and variance. Explain correctness-driven cost changes and investigate unexplained regressions.
- RustSec audit and dependency license inventory are reviewed; source repositories have a declared, compatible license and required notices are retained.
- Each binary has a SHA-256 sidecar and build/source/toolchain provenance. Verify after download before installation.
- Documentation, artifact versions and installation commands describe the candidate accurately.
- Recovery rehearsal passes: retain a previous verified binary when available; for the first release, remove the installed binary and restore project backups. `verify-candidate.py` rehearses project restoration without assuming an older release.
- Record a candidate-specific go/no-go report and resolve remaining blockers before publication.

Tag-triggered automation only creates a **draft prerelease** after the reusable CI gates pass. Publishing a stable release remains a separate, explicit release action. No tag or release is created by implementing this roadmap.
