# Context selection verification — 2026-09-18

The local candidate implements the canonical context-selection v1 contract. Full remains the new-project default; minimal is explicit and persists across upgrades. Mode changes preserve existing files and routes, including null routes, and report context-map conflicts.

## Source identity

- Catalog: `dc8902e3fcbaf521ec9ada022bf60f7da094e385`.
- Agent procedures: `5fceb09a50222a29ef0c9a52338ba9be9a82e995`.
- Model registry remains at its prior pin; no model guidance changed.
- Tested macOS x86_64 debug binary SHA-256: `0b8127460255715e8a4eb08a11786dabd2e1b5481bb5dd0c217f8d36ebacb7f5`.

These source commits were prepared locally and were not published during verification. Publish the catalog and agents commits before the dependent CLI branch; a fresh remote-only source sync cannot fetch unpublished commits. Local verification used clean checkouts at the exact pins, fetched from the corresponding local repositories with their official origin URLs retained.

## Executed checks

- `cargo test --locked --all-targets`: 144 tests passed. This includes seven context-selection integration tests and failure injection at every write boundary during minimal-to-full expansion. The catalog-selection unit test exercises both modes across packs, profiles, presets and skills.
- `cargo fmt --all -- --check` and `cargo clippy --locked --all-targets -- -D warnings` passed.
- `scripts/validate-contracts.py target/debug/ah`: actual contract output checks passed, including 17 context, 20 adapter and 16 skill-delivery probes. The older fixed skill fixture was reconciled with the new source pin's existing language review guide; all 14 payload hashes remain explicitly checked.
- `scripts/verify-candidate.py target/debug/ah`: copied binary outside the checkout passed with no Cargo or sibling binaries on PATH, including context selection, preservation and recovery checks.
- A temporary custom-prefix `install.sh --binary` installation generated all six variants in both modes with an empty runtime PATH. All 12 generated manifests and locks passed their pinned JSON Schemas, every lock checksum matched actual bytes, and all projects passed `ah validate`. The independent development schema check used the system Ruby YAML parser plus the declared Python jsonschema version; neither is a CLI runtime dependency.
- Canonical catalog/schema/public-surface checks, agents validation and exact source-identity checks passed. No dependency versions or workflow definitions changed.

## Measured generated context

These include default modules and skills, using the same source pins and selections in each comparison. Startup route means AGENTS.md, context README and manifest only; task-specific reads are additional.

| Variant | Profile | Files | UTF-8 bytes | Startup route bytes |
| --- | --- | ---: | ---: | ---: |
| base | minimal | 26 | 46,606 | 3,509 |
| base | full | 39 | 52,869 | 5,516 |
| web-app | minimal | 40 | 78,047 | 3,769 |
| web-app | full | 58 | 86,037 | 5,653 |

`scripts/context_probe.py` reproduces these measurements through the candidate verifier. Word/file/byte counts do not establish model effectiveness.

## Remaining limits

Linux, Windows and macOS arm64 CI were not executed for this change. This is debug-candidate evidence, not release artifact verification. Host loading, model outcomes, application behavior and the separate reviewed check executor remain unverified here. No issue was closed and no branch or release was published. The #86 application demonstration remains separate work.
