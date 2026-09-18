# Local executor correction evidence — 2026-09-18

CLI source `283817f` integrates the #61 ledger and corrects #62 process ownership and #63 invocation budgets on top of the #55 draft executor. Verification ran on macOS x86_64 with Rust 1.94.1. The remote draft PR #60 and issue statuses were not changed.

The tested debug binary SHA-256 is `bfede1e7a1b94c8014142e240538a52b80128e2a24d4dfd7c8086181cc6909c0`. Embedded canonical source is `e109cabb62521143cb4abe6c475dbe61d6a3d1d6`; agents source is `5fceb09a50222a29ef0c9a52338ba9be9a82e995`; model registry is `3b6635f036d4648dc7b4dc570df24ec1d509da51`. Local unpublished source pins prevent remote-only reproduction until publication.

## Observed checks

- `cargo test --locked --all-targets`: 174 passed, including 14 executable integration tests.
- `cargo clippy --locked --all-targets -- -D warnings` and `cargo fmt --check`: passed.
- Contract validation against the pinned canonical schemas: passed, including five actual execution cases and existing adapter, skill and context probes.
- `python3 scripts/verify-candidate.py target/debug/ah --report /tmp/ah-execution-candidate.json`: passed with the binary copied outside its checkout.
- The canonical #86 reading-list `tests/execution.py` trial passed unit, type, build and browser commands; all direct children were reaped. Stale review and missing native tool were refused. Total invocation 39,113 ms; review/revalidation 20,536 ms. These are local debug-build measurements, not performance targets.

Ledger tests retain earlier results after subsequent setup or revalidation failure and stop after optional integrity faults. Backend tests cover terminal observation without reaping, cleanup before collection, lost ownership, injected setup/read/signal/wait failures, bounded output, timeout and SIGINT/SIGTERM cancellation. Deadline tests include review reads and final revalidation. Tool hashing has a 256 MiB per-file and 512 MiB cumulative per-review cap; aliases share a cache only within one review.

The macOS cleanup state `no-live-group-members` records inspected absence of live group members when zombie-only group signaling returns EPERM. It does not classify arbitrary permission failures as successful cleanup.

## Remaining limits

Linux and macOS ARM execution have not been observed in this verification; Windows execution is unsupported. This is trusted-command execution without an OS sandbox. SIGKILL, supervisor crashes, detached descendants and uninterruptible OS calls are outside cleanup guarantees; fallback collection cannot guarantee reaping after supervisor exit.

Executable identity does not authenticate transitive dependencies or establish runtime versions. No imported-evidence freshness/authentication gate or native coding-host/model task was verified. `completion_verified` remains false even when all required commands pass. No release, remote CI result, deployment or issue closure is claimed.
