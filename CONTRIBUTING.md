# Contributing

Run `./scripts/sync-upstream.sh` before building from source.

Before opening a PR:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --all-targets
cargo test --all-targets
cargo run -- validate .
cargo run -- harness-audit upstream/agentic-harness/base
cargo run -- audit .
```

Canonical architecture/content changes belong in `agentic-harness`. Agent procedure changes belong in `agentic-harness-agents`. This repository should contain deterministic implementation, source pinning, tests, and release machinery only.
