# Verified-adoption integration with current main

Local candidate: `02bf3364625501e4e560742e35844c4987344c56`.

Merged current main `3787605` into `feat/verified-check-execution`, preserving the offline Decision Kernel and approved-check/completion command families. Canonical pin: `6b03603e9f0cfdb8348f5be4442622f8e8cdab0d`; agents pin: `7e7d44e9f9d7e4579048c4e8c1073243f6bcf61c`. Registry pin is unchanged.

The canonical merge preserves the Decision Kernel contracts, context selection and caller-approved completion contracts. The CLI merge resolves the source-pin conflict to that combined catalog. A Decision Kernel integration test previously hard-coded its initial catalog commit; it now verifies the complete canonical source identity against the checked-in lock entry.

## Local verification

macOS x86_64, Rust 1.94.1. Final `cargo test --locked --all-targets`: 197 passed, zero failures. Includes 14 native executor integration tests, 16 governance semantic tests and the offline Decision Kernel behavior tests. Formatting and Clippy with warnings denied pass. The first combined run exposed only the outdated hard-coded provenance expectation; the corrected CLI suite and then the full suite passed.

Debug binary SHA-256: `50813c77a0d593e1d9015ad98a28071833f55bb99525227f82b7a7217c1f621c`.

Canonical catalog and combined CLI contract validators pass, including both completion and Decision Kernel schemas. Actual-output contract verification also passes: 20 adapter probes, 16 skill-delivery probes, 17 context-selection probes, execution reports and 25 caller-approved completion cases. The latter include acceptance with required governance and rejection of stale/tampered/missing/changed/unapproved evidence, traversal and symlinks. These are local macOS debug-binary results, not platform-matrix or model outcomes.

## Delivery and release limits

Source remains local. Upstream commits must be published before a fresh remote-only CLI build can resolve these pins. The original remote draft PR #60 has not been updated or replaced yet. Fresh platform CI/review is required before closing #55/#61/#62/#63 and companion canonical #84/#85. Windows command execution remains unsupported.

The earlier optimized macOS candidate/benchmark remains evidence for its own source identity, not for this merged debug candidate. No fresh optimized release, platform matrix, RustSec audit or release publication is claimed. Native-host adoption remains blocked by the recorded Claude authentication failure; no credentials were inspected and no trial was rerun.
