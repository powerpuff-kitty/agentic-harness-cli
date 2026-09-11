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

## Attribution and artifact verification

`scripts/dependency-notices.py --output notices/THIRD_PARTY_NOTICES.txt` collects original attribution from every resolved Cargo dependency and the installed Rust standard-library distribution. The generated JSON records package versions, source/file hashes, Cargo lock identity, toolchain identity and embedded-source pins. Missing authored-source license declarations remain explicit. Omitted upstream license files are supplemented from exact crate VCS commits under `third-party/`; changed pins or text fail collection.

Package with `python3 scripts/package-candidate.py BINARY --commit FULL_SHA --notices notices/THIRD_PARTY_NOTICES.txt`. Each platform ZIP contains the binary, binary checksum, provenance v2, dependency notice text/metadata and Rust library notices; the ZIP has its own checksum. ZIP timestamps and permissions are normalized. Identical inputs produce identical archives on the same packaging environment; cross-platform toolchain or compression differences may change bytes.

Run `python3 scripts/package-candidate.py ARCHIVE.zip --verify-archive` before extraction. This checks archive integrity, exact allowed members, bounded expansion and the enclosed binary/notice hashes without executing any target-platform code. `--verify` on an extracted binary checks all hashes before invoking `--version`. Checks remain active under Python optimization. Checksums detect changes relative to the downloaded sidecars; they are not signatures or an independent source-authentication mechanism.

CI candidate validation and tag-triggered draft preparation use `--require-authored-licenses`. Undeclared CLI/package, canonical, model-registry or agent-source licenses fail these checks. The owner approved MIT for authored CLI, canonical/registry and agent content (canonical ADR-007). Preserve third-party terms and review compatibility/attribution when inputs change; file presence and SPDX collection alone are not legal approval. Published candidate assets consist of complete ZIPs and ZIP checksums so attribution travels with the binary.

Generated projects retain copied Harness attribution in `.agentic/THIRD_PARTY_NOTICES.md`. Initialization and upgrades preserve independently authored application licensing. The model-registry source is checked separately even when it shares a repository and revision with the canonical catalog.
