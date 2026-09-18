# Architecture

## Public ecosystem boundaries

```text
agentic-harness
canonical project contract + catalog + schemas + registries
        ↓ pinned source
agentic-harness-agents
skills, prompts, host adapters and procedures
        ↓ pinned source
agentic-harness-cli
native composition, analysis, validation and explicit artifact gates
        ↓
self-contained target repository / versioned project artifacts
```

The canonical repository owns project contracts. The agents repository owns procedures. The CLI owns deterministic mechanics. No downstream layer may silently redefine upstream contracts or accepted target-project truth.

Public documentation and diagrams enumerate approved public repositories only; see [public-surface policy](docs/project/public-surface.md).

## Design-intelligence boundary

Design Genome, Design Analysis and related public schemas live under `catalog/schema/`. Consumers establish compatibility through versioned contracts and shared fixtures rather than depending on a particular interface implementation. The CLI's deterministic core must not require hosted services.

The implemented static workflow is analysis → review-required candidate → approved identity/task context compilation → measured drift comparison. Runtime analysis and additional compiler outputs remain planned unless the installed CLI contract explicitly supports them. External reference data and AI interpretations remain evidence, not accepted project intent.

## Decision-intelligence boundary

The provider-neutral [Decision Kernel contract](docs/architecture/decision-kernel.md), accepted in [ADR-008](decisions/ADR-008-decision-kernel.md), separates immutable state/evidence, atomic decision specifications, provider execution, policy, receipts, outcomes and side-effect-free evaluation.

Jev, deterministic rules, statistical models, LLMs and humans may act as decision providers when their declared capabilities fit the DecisionSpec. Providers never own consequence authorization. Provider confidence, calibration, evidence coverage, evidence reliability, decision certainty and domain outcome probability are separate quantities.

Decision graphs describe dependencies between atomic decisions and deterministic reducers; they do not replace workflow engines. DecisionPolicy remains application-owned, and high-consequence actions require separate authorization/review according to project policy. DecisionReceipt preserves what was known and produced at decision time; later observations append DecisionOutcome rather than rewriting history.

The canonical schemas live under `catalog/schema/decision-*.v1.schema.json`. The CLI may implement deterministic validation, provider adapters, fan-out, cache/replay and executable policy checks, but published schemas alone are not evidence that those runtime capabilities are installed or enforced.

## Target-project contract

```text
project/
├── README.md
├── AGENTS.md
├── source and normal project files
├── .agentic/
│   ├── README.md
│   ├── manifest.yaml
│   ├── lock.json
│   ├── PRODUCT.md
│   ├── ARCHITECTURE.md
│   ├── SECURITY.md
│   ├── optional DESIGN.md and REFERENCE.md
│   ├── decisions/
│   ├── plans/
│   ├── tasks/
│   ├── docs/
│   ├── evals/
│   ├── packs/
│   └── policies/
└── .agents/skills/
```

Only the compact `AGENTS.md` router is mandatory at the project root. Vendor-required files must refer to canonical context and preserve native host semantics. A pointer's existence alone is not proof that the host loaded it.

## Authoring catalog

`catalog/variants/<name>/files/` contains an inspectable materialized project-context tree, not a runnable application. Packs, policies, profiles, presets and schemas remain independently reusable. Existing variant IDs and `--boilerplate` arguments remain unchanged by the public terminology clarification.

`catalog/quick-start.json` records the public README's onboarding argument arrays. The catalog validator checks consistency without executing Markdown commands. The CLI must separately smoke-test those examples against its exact pinned sources and installed binary.

## Composition and compatibility

Implemented composition selects a variant/preset/profile, installs context and selected modules, records source identities/checksums, preserves project-authored files and reports conflicts. Refer to the CLI's version-specific contract for exact behavior. Comprehensive generated vendor adapter synchronization is tracked work, not a current universal capability.

Legacy root-level context can be inspected for compatibility. Automatic filesystem migration is not implemented in the current CLI. Use the [backup-first manual procedure](docs/project/migration-v1.md); the model-profile migration preview does not move files.

## Enforcement and verification boundary

Declared policy, delivered instructions, executed checks and enforced restrictions require distinct evidence. A manifest is not a sandbox. The current audit discovers checks without executing them; an artifact gate applies explicitly selected conditions and does not establish application test success.

The planned evidence model and approved check runner must preserve source/config identity, freshness, failure/skip states and host limitations. Do not change existing artifact meanings silently while implementing them.

## Invariants

- Project truth remains local, inspectable and self-contained.
- Policies retain explicit precedence; skills remain procedures rather than architecture authority.
- Project-specific content is preserved; source-only metadata does not leak into generated projects.
- Actual destructive, production and publication mechanisms require appropriate approval; declarations alone are not evidence of enforcement.
- Reports state what was measured, skipped, unsupported or not executed.
- Deterministic operations do not introduce unexplained drift.
- Design identity becomes project truth only through explicit acceptance; observed drift alone is not a quality judgment.
- Provider rights/retention restrictions must be verified before restricted ingestion or redistribution.
- Decision providers cannot grant consequence authority; semantic/model output remains advisory input until deterministic project policy and any required human/authoritative review are satisfied.
- Evaluation, shadow and counterfactual runs are side-effect free; recorded provider confidence is never silently reinterpreted as domain outcome probability.

## Authored-source licensing

Authored CLI, catalog/registry and agent content use MIT, as accepted in [ADR-007](decisions/ADR-007-mit-licensing.md). Third-party licenses remain intact. Variants retain `.agentic/THIRD_PARTY_NOTICES.md` without assigning a license to independently authored application code. Composition preserves project license files and records copied-notice provenance.
