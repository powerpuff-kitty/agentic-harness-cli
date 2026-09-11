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
