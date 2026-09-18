# macOS candidate review — 2026-09-18

Disposition: **no-go for release publication**. The local optimized macOS x86_64 candidate passes the checks below; current cross-platform evidence and a fresh RustSec audit are unavailable. No release, tag, source push or remote CI run was created.

## Candidate identity

- CLI source: `f01797305d825dcc9c19fedee6758c02aa9a0a16` (includes completion implementation `1f08b82`).
- Canonical pin: `eac2d48`; agents pin: `7e7d44e`; model registry pin unchanged at `3b6635f036d4648dc7b4dc570df24ec1d509da51`.
- Toolchain: `rustc 1.94.1 (e408947bf 2026-03-25)`; macOS 15.8 x86_64; Python 3.14.7.
- Build: `cargo build --locked --release --bin ah`.
- Optimized binary SHA-256: `84fe9ae9dfa466ba792b2b01651c2b60fabe1c3d58c391752a91377103c4c216`.
- ZIP SHA-256: `11c14e7d74be6ae43c2a1f6cfe96087f94ad1b0dd040e1deabe762f32529df27`.

The package includes binary/checksum, provenance v2, dependency notice text/metadata and Rust library notices. Attribution collection covers 122 dependency packages with no undeclared authored-source licenses. Collection and integrity checks do not constitute a new legal review or producer signature.

## Gate evidence

| Gate | Observed result |
| --- | --- |
| Locked optimized build | Passed |
| Rust suite / formatting / Clippy | Existing same-source verification: 190 tests passed; formatting and warnings-denied Clippy passed; not repeated for a documentation-only review |
| Actual optimized output schemas | Passed, including execution and caller-approved completion |
| Copied optimized binary outside checkout | Passed; 25 completion cases plus existing command, onboarding, adapter, skill/context and recovery probes |
| Custom-prefix installation and source launcher | Both passed 17 onboarding, 20 adapter and 16 skill-delivery probes; failed-install preservation and invalid command-name rejection passed |
| Archive and extracted-binary integrity | Strict authored-license/provenance/checksum verification passed; nine packaging negative regressions passed |
| Benchmark | One warmup + five fresh-process samples for each of 12 workloads; observations below |
| Fresh RustSec audit | Not run: `cargo audit` is not installed; earlier candidate audits cannot certify these bytes |
| Linux and macOS ARM/Windows candidate evidence | Not observed for this source; local Docker daemon unavailable; Windows execution remains unsupported |
| Public reproducibility | Blocked until local canonical/agent/CLI commits are published and exact pinned inputs are remotely available |
| Native coding-host/model outcome | Separate prepared #86 trial; no model invocation or native-loading/enforcement claim |

Installation tests use disposable prefixes and existing fixture backup/restore checks. There is no prior production binary rollback claim. The caller-approved completion verdict remains limited to declared checks and required controls; signed producers are deferred per ADR-010.

## Benchmark observations

Raw samples and environment are retained in [the benchmark record](2026-09-18-macos-benchmark.json). Workloads are synthetic Vue chains (first nine rows) and identical skill trees (last three). Other validation ran concurrently for part of this measurement. No same-environment prior baseline was supplied, so these observations do not establish a regression, an improvement or a service-level objective. RSS follows the documented wait4 accounting and is not aggregate concurrent-worker memory.

| Size | Command | Median seconds | Sample standard deviation |
| --- | --- | --- | --- |
| 100 | audit | 0.0819 | 0.0071 |
| 100 | architecture analyze | 0.0446 | 0.0076 |
| 100 | design-system-components | 0.0274 | 0.0110 |
| 1000 | audit | 0.8835 | 0.2661 |
| 1000 | architecture analyze | 0.5172 | 0.0861 |
| 1000 | design-system-components | 0.3182 | 0.0282 |
| 5000 | audit | 3.3333 | 1.3883 |
| 5000 | architecture analyze | 1.6338 | 0.3859 |
| 5000 | design-system-components | 0.6322 | 0.0619 |
| 10 | agentic skills | 0.0098 | 0.0003 |
| 100 | agentic skills | 0.0233 | 0.0004 |
| 500 | agentic skills | 0.0947 | 0.0174 |

## Reproduction

Run the existing `scripts/validate-contracts.py`, `verify-candidate.py`, `verify-install.py`, `dependency-notices.py`, `package-candidate.py`, `test-artifacts.py` and `benchmark.py` against the locked optimized binary. Packaging must use the exact candidate commit and collected notices; verify archives before executing extracted binaries. Full commands and attribution semantics remain in [the release checklist](../release-checklist.md).

Before publication, make pinned sources available, run the existing platform CI, perform a current RustSec audit and complete candidate review. Stable release authorization remains separate. No inferred approval or historical green result removes these blockers.
