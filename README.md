# Agentic Harness CLI

Native Rust `ah` CLI for deterministic Agentic Harness composition, audits, validation, security checks, design-system compliance, and quality gates.

The CLI intentionally does **not** own canonical boilerplates or agent procedures:

- canonical catalog: https://github.com/powerpuff-kitty/agentic-harness
- agent skills/prompts: https://github.com/powerpuff-kitty/agentic-harness-agents

It pins exact revisions of both repositories and embeds their content into release binaries, so generated projects and binary users do not require network access or Rust at runtime.

## Source build

```bash
./scripts/sync-upstream.sh
cargo test --all-targets
cargo build --release
```

The pinned canonical catalog exposes complete root boilerplates (`base`, `web-app`, `backend-api`, `saas`, `monorepo`, `library-sdk`) and shared modules under `modules/`.

## Commands

```bash
ah init ./app --boilerplate web-app
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

`--boilerplate` is the preferred project-shape flag. `--template` remains a backward-compatible alias for existing automation.

## Repository role

```text
agentic-harness          canonical complete boilerplates + modules
agentic-harness-agents   agent procedures
        ↓ pinned snapshots
agentic-harness-cli      deterministic engine
        ↓
self-contained target project
```
