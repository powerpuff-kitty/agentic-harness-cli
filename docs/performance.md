# Performance evidence

Run `python3 scripts/benchmark.py target/release/ah --output /tmp/benchmark.json`. Add `--target PATH` for a read-only representative repository. Default workloads cover 100/1,000/5,000 Vue components with a dependency chain and 10/100/500 identical skills. Reports retain source identity, binary checksum, environment, one warmup, five fresh-process measurements, standard deviation, output size and per-child peak RSS (where `wait4` is available). No runtime SLO is inferred from these samples.

`--baseline prior.json` compares the same workload and environment. The median allowance is the largest of 5 ms, 15% of the previous median, and three times the sum of observed standard deviations. The 15% floor accommodates the approximately 10% local warm-cache variability seen during this audit; retain raw samples and investigate noisy runs. Cross-environment comparisons are refused. Output shape or coverage changes require review before accepting a replacement baseline.

## Correctness-fix comparison

On the original 2026-09-09 fixture and the same macOS arm64 / M1 Max / 32 GiB / Rust 1.94.1 environment, three measured runs after one warmup gave:

| Workload | Original median | Corrected median |
| --- | ---: | ---: |
| Audit, 5,000 files | 1.608 s | 0.638 s |
| Architecture detection, 5,000 files | 0.053 s | 0.048 s |
| Architecture source analysis, 5,000 files | 0.200 s | 0.258 s |
| Static design analysis, 5,000 files | 0.313 s | 0.253 s |
| Agentic audit, 500 distinct skill files | 0.299 s | 0.086 s |

Audit improves through a shared inventory and single-pass design discovery. Skill comparisons precompute word sets and deduplicate identical content. Source analysis adds a full syntax tree and source-kind classification; its additional approximately 58 ms on this fixture accompanies deliberate parser/coverage corrections. Findings are not expected to remain identical where the old scanner counted generated code or mistook types/comments for UI/imports. The parser regressions demonstrate the corrected interpretation.

A separate five-run dependency-chain workload measured a 5,000-file audit median of 0.891 s and source analysis of 0.431 s. A read-only Lahaku run measured audit 4.146 s, source analysis 3.833 s and design-component discovery 0.113 s. This realistic source-graph cost remains visible rather than inferred from the faster synthetic fixture. These observations were captured during implementation; candidate-specific reports should supersede them for release decisions.

The original baseline and comparison samples are retained in `docs/evidence/`. Candidate benchmarks should run without concurrent compilation or other benchmark jobs. CI platform timings are separate baselines from developer-laptop observations.

## TypeScript parser correction (2026-09-11)

A same-session comparison on macOS arm64 / M1 Max / Rust 1.94.1 used the old `8c3e2eb` binary and the Oxc candidate, one warmup and five fresh processes per workload. Raw samples, hashes, variance and output sizes are in [before](evidence/benchmark-parser-before-20260911.json) and [after](evidence/benchmark-parser-after-20260911.json).

| Workload | Before median ± sample SD | After median ± sample SD | Before / after peak RSS |
| --- | ---: | ---: | ---: |
| Lahaku audit, 4,702 source files | 3.717 ± 0.022 s | 1.587 ± 0.053 s | 78.63 / 76.50 MiB |
| Lahaku architecture analysis | 3.425 ± 0.028 s | 1.263 ± 0.018 s | 74.08 / 70.66 MiB |
| Vue chain audit, 5,000 files | 0.923 ± 0.009 s | 0.974 ± 0.027 s | 27.61 / 28.31 MiB |
| Vue chain architecture analysis | 0.451 ± 0.009 s | 0.485 ± 0.013 s | 22.28 / 22.42 MiB |

Replacing Tree-sitter with Oxc removes 271 parser-gap files from the representative scan. Source scope (4,702 files), local edges (19,458), resource imports (117) and the runtime cycle finding remain stable. All 53 remaining gaps are imports into deliberately excluded source. Scoring therefore remains unavailable. Audit output shrinks from 5,524,318 to 5,449,936 bytes because obsolete parser-gap evidence is removed; extra contract text accounts for small synthetic-output growth.

The new implementation includes a persistent parser worker to contain stack exhaustion. Larger real TypeScript files benefit substantially, while the small-file chain pays IPC overhead (5.5% audit / 7.4% architecture median increase). Every workload passes the stated variance-aware regression allowance. The stripped arm64 binary shrinks from 5,861,456 to 3,617,024 bytes.

RSS is the maximum reported by `wait4` across five samples, with parser workers reaped before normal exit. It follows OS child-resource accounting and is **not** a sum of simultaneous parent/worker resident memory. Timings include worker startup, communication and teardown. These warm-cache laptop measurements do not establish cold-cache or cross-platform SLAs.
