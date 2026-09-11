# Changelog

## Unreleased

- Embed the current canonical catalog and model registry in the unified `ah` command surface.
- Validate real project YAML/context routes, audit artifacts, thresholds and arguments; preserve explicit legacy reads.
- Stage composition, preserve custom files, report conflicts, record source/checksum provenance and restore managed metadata after recoverable failures.
- Share confined, ignore-aware inventories; exclude generated/test/tooling content from product metrics.
- Parse JS/TS/TSX and Vue/Svelte scripts; distinguish runtime/type/dynamic imports, resolve common aliases/workspace exports and honor exception expiry.
- Remove invented quality/readiness scores; publish unmeasured values and unsupported coverage explicitly in audit v2.
- Use template-aware design discovery and bounded, redacted secret markers.
- Add CLI regression tests, installed-artifact checks, repeatable performance tooling and draft-only candidate releases.

Compatibility: consumers must handle explicit null scores in audit v2. Unknown flags and malformed inputs now fail. `validate` checks its target, and no longer validates embedded templates on an invalid target. Use `catalog-check` for bundled content. Legacy filesystem upgrades require explicit migration.
