# Agentic Harness CLI

[![Status: Beta](https://img.shields.io/badge/status-beta-orange)](https://github.com/powerpuff-kitty/agentic-harness-cli)
[![CLI CI](https://github.com/powerpuff-kitty/agentic-harness-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/powerpuff-kitty/agentic-harness-cli/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/Rust-native-000000?logo=rust)](https://www.rust-lang.org/)

**Native Rust `ah` CLI for composing, validating, auditing, and governing agent-native repositories used with Codex, Claude Code, Cursor, GitHub Copilot, Gemini CLI, and other coding agents.**

> **Status: Beta.** The CLI is pre-1.0: core commands are usable, but command and schema compatibility may still evolve before the first stable release.

## Installation

### Install from source

The current installer supports a local source checkout on macOS and Linux. You need [Git](https://git-scm.com/), Python 3, and a working [Rust toolchain](https://rustup.rs/) (CI uses 1.94.1).

```bash
git clone https://github.com/powerpuff-kitty/agentic-harness-cli.git
cd agentic-harness-cli

./scripts/sync-upstream.sh
./install.sh
```

By default, `install.sh` builds the release binary when needed and installs `ah` to `/usr/local/bin/ah`.

If `/usr/local` is not writable for your user, either run the installation with appropriate permissions or install into a user-owned prefix:

```bash
./install.sh --prefix "$HOME/.local"
```

When using `~/.local`, make sure `~/.local/bin` is on your `PATH`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

Verify the installation:

```bash
ah --help
```

The installer can also install an already-built binary:

```bash
cargo build --locked --release --bin ah
./install.sh --binary ./target/release/ah
```

A custom command name can be selected with `--command`:

```bash
./install.sh --command agentic-harness
```

### Release binaries

Release binaries are intended to be the preferred installation path once GitHub Releases are published. They embed pinned snapshots of the canonical `agentic-harness` and `agentic-harness-agents` repositories, so binary users will not need GitHub access or Rust at runtime.

Until release artifacts are available, use the source installation above.

## Quick start

```bash
ah init ./app --boilerplate web-app
ah audit ./app
ah validate ./app
```

Agentic Harness separates project truth, agent behavior, and deterministic enforcement:

```text
agentic-harness          canonical boilerplates + modules + public schemas
agentic-harness-agents   skills + prompts + adapters
        ↓ pinned snapshots
agentic-harness-cli      native Rust `ah` engine
        ↓
self-contained target project
```

- **[agentic-harness](https://github.com/powerpuff-kitty/agentic-harness):** canonical architecture, complete boilerplates, reusable modules, and public machine contracts
- **[agentic-harness-agents](https://github.com/powerpuff-kitty/agentic-harness-agents):** agent-facing skills, prompts and workflows
- **This repository:** deterministic composition, audits, validation, security checks, architecture/design analysis, and quality gates

Release binaries embed pinned snapshots of the canonical and agent repositories, so generated projects and binary users do not require GitHub access or Rust at runtime.

Audit v2 reports `overall` and unmeasured quality/readiness scores as `null`. File presence does not establish test quality or production readiness. Gates validate evidence before applying explicit policy. See [command/compatibility contracts](docs/cli-contracts.md), [performance evidence](docs/performance.md), and [release gates](docs/release-checklist.md).

New projects use root `AGENTS.md` and `.agentic/manifest.yaml` with routed context. Upgrades preserve custom files and report conflicts; legacy `agentic.yaml` layouts require an explicit migration. All command families and the pinned model registry are embedded in the installed `ah` binary.

## Commands

```bash
ah init ./app --boilerplate web-app
ah init ./saas --preset vue-saas --profile startup
ah upgrade ./existing --profile enterprise
ah audit .
ah architecture detect .
ah architecture analyze .
ah architecture analyze . --profile pattern/feature-first/1
ah design-system-components . --write
ah validate .
ah security-scan .
ah harness-audit .
ah compare before.json after.json
ah gate audit.json --max-architecture-errors 0
```

`--boilerplate` is the preferred project-shape flag. `--template` remains a backward-compatible alias for existing automation.

## Experimental architecture intelligence

Architecture intelligence is offline and deterministic.

### Detect

```bash
./ah architecture detect ./my-app
```

Detection finds supported framework markers (currently Vue, Nuxt, Angular, React and Next), separates build/workspace tooling such as Vite and Nx from application frameworks, detects ecosystem packages such as Pinia and Vue Router, and classifies visible source structure as feature-first, technical-layered, layered, hexagonal/clean, mixed or unknown.

Detection output includes evidence, confidence and candidate Architecture Registry profile IDs. Unknown or mixed projects are intentionally left unresolved rather than being forced into a generic folder template.

### Analyze

```bash
./ah architecture analyze ./my-app
./ah architecture analyze ./my-app --profile pattern/feature-first/1
```

Architecture Analysis v1 currently builds a local JS/TS/Vue source dependency graph and reports source-file/import/line evidence for:

- import cycles
- Nuxt `app/` ↔ `server/` boundary violations
- Nuxt `shared/` importing app/server-only code
- cross-feature imports that reach into another feature's internals instead of its root public interface
- shared/common code importing feature-owned internals
- reverse dependencies between presentation, application, domain and infrastructure layers when the layered profile is selected or detected

The analyzer resolves relative imports and built-in `@/`, `~/` and `#shared/` aliases. Unresolved local imports are reported separately rather than silently treated as external packages. Findings carry stable Architecture Registry rule IDs plus authority and deterministic/heuristic classification.

The report also explicitly lists what is not yet checked, including non-JS language graphs, arbitrary custom alias maps, computed runtime imports, semantic business-logic placement and project-local exceptions.

Project-local architecture contracts, general registry rule compilation, ESLint/Nx/dependency-cruiser adapters, additional languages and integration into the main audit score remain follow-up work.

## Experimental design intelligence

The first closed-loop design workflow is deterministic and does not require an LLM:

```bash
./ah design analyze ./my-app --level static --output design-analysis.json
./ah design preserve --analysis design-analysis.json --output design-genome.candidate.json
./ah design prompt --genome design-genome.approved.json --task design-task.json --output implementation-brief.md
# implement the task, then analyze again
./ah design diff before.json after.json --output design-diff.json
```

### Analyze

The static analyzer measures source-level evidence for hexadecimal color usage, `font-size` pixel values, margin/padding/gap values, border radii, and CSS custom-property definitions/references. It emits Design Analysis format v1 and explicitly lists runtime/visual checks that were **not** performed.

### Preserve

`design preserve` transforms a Design Analysis v1 artifact into a **candidate**, review-required Design Genome. It preserves measured values, counts, source evidence, analysis findings, token observations, and the unverified-check boundary. It deliberately does **not** infer brand identity from frequency, does not auto-approve design rules, and marks intent that cannot be established from static evidence as unknown.

The candidate includes a review queue for promoting only intentional evidence into an approved Design Genome. Approval remains a human/project decision.

### Compile

An approved Design Genome plus a structured Design Task can be compiled into a model-neutral implementation brief without calling an LLM:

```bash
./ah design prompt \
  --genome design-genome.json \
  --task design-task.json \
  --output implementation-brief.md
```

The compiler selects only rules whose scope matches the task's mode/surface/page/component/state/breakpoint context, orders required guidance before advisory guidance, includes requested approved component contracts, and reports unresolved component names instead of inventing contracts.

### Verify

Two Design Analysis artifacts can be compared without AI:

```bash
./ah design diff before.json after.json
```

The diff reports new/removed measured values, changed usage counts, finding IDs that appeared/disappeared, and changes in performed/not-checked verification. These are **drift observations**, not a subjective quality or originality score.

Runtime contrast, responsive layout, accessibility evidence, richer token/component health, interactive Design Genome approval, and model-specific prompt adapters remain follow-up work.

The source launcher currently routes `design` to the experimental `ah-design` binary. Stable release-binary command integration will be completed before the design command is promoted from experimental status.

## What `ah` provides

- deterministic boilerplate composition
- profile, pack, policy and skill installation
- existing-project upgrades that preserve project-specific truth
- codebase and harness audits
- experimental framework/architecture detection and deterministic import-boundary analysis
- design-system component planning and structural compliance checks
- experimental deterministic design analysis, candidate preservation, drift comparison, and prompt compilation
- baseline secret scanning
- machine-readable validation and quality gates
- self-contained native binaries for supported release platforms

## Source build

```bash
./scripts/sync-upstream.sh
cargo test --all-targets
cargo build --locked --release --bin ah
```

The pinned canonical catalog exposes complete root boilerplates (`base`, `web-app`, `backend-api`, `saas`, `monorepo`, `library-sdk`) and shared modules under `modules/`.

## Contributing

The CLI should contain deterministic mechanics rather than canonical architecture or large prompt collections. Architecture/content changes belong in `agentic-harness`; agent procedure changes belong in `agentic-harness-agents`.
