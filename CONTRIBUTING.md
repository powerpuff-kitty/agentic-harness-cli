# Contributing

Source builds require Git, Python 3 and Rust. CI fixes Rust at 1.94.1. Run `./scripts/sync-upstream.sh` before building; it refuses to overwrite edited source inputs.

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo run --locked -- catalog-check
cargo build --locked --release --bin ah
python3 scripts/verify-candidate.py target/release/ah --report /tmp/candidate.json
python3 scripts/benchmark.py target/release/ah --output /tmp/benchmark.json
cargo audit
```

The CLI source repository is not a generated harness project: `ah validate .` must report a missing project manifest here. `catalog-check` checks embedded content; integration tests generate and validate real projects. Tests cover the installed command surface, positive/negative artifacts and arguments, composition preservation and injected write failures, current/custom/legacy layouts, parser/resolution regressions, expiry, scanner confinement, design and secret redaction.

CI builds Linux x86_64, macOS x86_64/arm64 and Windows x86_64 candidates, downloads their artifacts into separate jobs, and exercises copied binaries outside the checkout with no Cargo on PATH or runtime upstream inputs. Python scripts are development/release tools, not CLI runtime dependencies.

See [performance methodology](docs/performance.md), [public contracts](docs/cli-contracts.md), and [release gates](docs/release-checklist.md). Canonical schemas belong in `agentic-harness`; procedures belong in `agentic-harness-agents`. This repository owns implementation, source pins, tests and release machinery.
