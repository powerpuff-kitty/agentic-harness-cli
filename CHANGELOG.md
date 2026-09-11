# Changelog

## Unreleased

- License authored content under MIT and retain copied Harness attribution without licensing generated application code.

- Embed the current canonical catalog and model registry in the unified `ah` command surface.
- Validate real project YAML/context routes, audit artifacts, thresholds and arguments; preserve explicit legacy reads.
- Stage composition, preserve custom files, report conflicts, record source/checksum provenance and restore managed metadata after recoverable failures.
- Share confined, ignore-aware inventories; exclude generated/test/tooling content from product metrics.
- Replace the limited TypeScript grammar with Oxc, compiler-checked fixtures and crash-contained native parsing.
- Parse JS/TS/TSX and Vue/Svelte scripts; distinguish runtime/type/dynamic imports, resolve common aliases/workspace exports and honor exception expiry.
- Remove invented quality/readiness scores; publish unmeasured values and unsupported coverage explicitly in audit v2.
- Use template-aware design discovery and bounded, redacted secret markers.
- Package reproducible ZIP bundles with pinned dependency/Rust library attribution; verify all hashes before executing downloaded code, and block draft creation while authored licenses are undeclared.
- Add CLI regression tests, installed-artifact checks, repeatable performance tooling and draft-only candidate releases.

Compatibility: consumers must handle explicit null scores in audit v2. Unknown flags and malformed inputs now fail. `validate` checks its target, and no longer validates embedded templates on an invalid target. Use `catalog-check` for bundled content. Legacy filesystem upgrades require explicit migration.
