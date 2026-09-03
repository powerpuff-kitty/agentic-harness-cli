# Agentic Harness CLI

Native Rust `ah` CLI for deterministic Agentic Harness composition, audits, validation, security checks, design-system compliance, and quality gates.

The CLI intentionally does **not** own canonical boilerplates or agent procedures:

- canonical boilerplate/catalog source: https://github.com/powerpuff-kitty/agentic-harness
- agent skills/prompts: https://github.com/powerpuff-kitty/agentic-harness-agents

Release binaries embed pinned snapshots of both sources, so end users do not need network access or Rust at runtime.

## Source checkout

A source build needs the pinned upstream repositories under `upstream/`. Run:

```bash
./scripts/sync-upstream.sh
cargo test
cargo build --release
```

CI and release workflows check out the pinned upstream revisions automatically.

## Commands

```bash
ah init ./app --template web-app
ah init ./saas --preset vue-saas --profile startup
ah upgrade ./existing --profile enterprise
ah audit .
ah design-system-components . --write
ah validate .
ah security-scan .
ah harness-audit .
ah compare before.json after.json
ah gate audit.json --min-overall 80 --min-score security=80 --min-score design_system=85
```

`--template` is currently retained as the CLI compatibility flag for selecting a canonical boilerplate. Public/source terminology is **boilerplate**; future CLI evolution can add a `--boilerplate` alias without breaking existing automation.

## Repository role

```text
agentic-harness          canonical truth + boilerplates
agentic-harness-agents   agent procedures
        ↓ pinned snapshots
agentic-harness-cli      deterministic engine
        ↓
self-contained target project
```
