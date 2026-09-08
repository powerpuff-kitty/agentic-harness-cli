# Agentic Harness CLI

[![Status: Beta](https://img.shields.io/badge/status-beta-orange)](https://github.com/powerpuff-kitty/agentic-harness-cli)
[![CLI CI](https://github.com/powerpuff-kitty/agentic-harness-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/powerpuff-kitty/agentic-harness-cli/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/Rust-native-000000?logo=rust)](https://www.rust-lang.org/)

**Native Rust `ah` CLI for composing, validating, auditing, and governing agent-native repositories used with Codex, Claude Code, Cursor, GitHub Copilot, Gemini CLI, and other coding agents.**

> **Status: Beta.** The CLI is pre-1.0: core commands are usable, but command and schema compatibility may still evolve before the first stable release.

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
- **This repository:** deterministic composition, audits, validation, security checks, design analysis, and quality gates

Release binaries embed pinned snapshots of the canonical and agent repositories, so generated projects and binary users do not require GitHub access or Rust at runtime.

## Commands

```bash
ah init ./app --boilerplate web-app
ah init ./saas --preset vue-saas --profile startup
ah upgrade ./existing --profile enterprise
ah audit .
ah architecture detect .
ah design-system-components . --write
ah validate .
ah security-scan .
ah harness-audit .
ah compare before.json after.json
ah gate audit.json --min-overall 80 --min-score security=80 --min-score design_system=85
```

`--boilerplate` is the preferred project-shape flag. `--template` remains a backward-compatible alias for existing automation.

## Experimental architecture intelligence

Architecture detection is offline and deterministic:

```bash
./ah architecture detect ./my-app
```

It detects supported framework markers (currently Vue, Nuxt, Angular, React and Next), separates build/workspace tooling such as Vite and Nx from application frameworks, detects ecosystem packages such as Pinia and Vue Router, and classifies visible source structure as feature-first, technical-layered, layered, hexagonal/clean, mixed or unknown.

Detection output includes evidence, confidence and candidate architecture profile IDs. Unknown or mixed projects are intentionally left unresolved rather than being forced into a generic folder template. This is the first slice of the Architecture Registry/analyzer work; import-graph validation, dependency-boundary enforcement and generated ESLint/Nx/dependency-cruiser adapters are follow-up work.

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
- experimental framework/architecture detection with evidence and confidence
- design-system component planning and structural compliance checks
- experimental deterministic design analysis, candidate preservation, drift comparison, and prompt compilation
- baseline secret scanning
- machine-readable validation and quality gates
- self-contained native binaries for supported release platforms

## Source build

```bash
./scripts/sync-upstream.sh
cargo test --all-targets
cargo build --release
```

The pinned canonical catalog exposes complete root boilerplates (`base`, `web-app`, `backend-api`, `saas`, `monorepo`, `library-sdk`) and shared modules under `modules/`.

## Contributing

The CLI should contain deterministic mechanics rather than canonical architecture or large prompt collections. Architecture/content changes belong in `agentic-harness`; agent procedure changes belong in `agentic-harness-agents`.
