# CLI architecture

The shared Rust library owns command parsing, composition, project validation, scoped inventory, and analyzers. `ah` dispatches every family directly; the `ah-agentic`, `ah-architecture`, and `ah-design` binaries are compatibility entry points into the same library.

Build inputs are pinned in `upstream.lock.json`: canonical `catalog/variants/*/files`, packs, policies, profiles, presets and schemas; agent procedures/skills; and model-registry profiles. `include_dir` embeds these inputs. Runtime operation requires no source checkout, Rust, registry directory, network or sibling binaries. `scripts/sync-upstream.py` preserves existing source edits and fetches exact revisions; `Cargo.lock` fixes Rust dependencies.

Composition resolves selections before writes, stages complete current-layout projects, and publishes new targets with a directory rename. Existing targets retain custom content and report conflicts. Added files use exclusive creation; managed manifest/lock replacements are atomic per file and restored on recoverable write failures. This is not a crash-wide filesystem transaction. Source commits and SHA-256 checksums record installed provenance. Upgrades retain original checksums for preserved files to keep customization detectable.

`project.rs` reads current YAML and legacy YAML explicitly. Current context routes resolve relative to the manifest directory and must remain inside the selected repository. Legacy composition is refused with a migration diagnostic; inspection remains available. Canonical format authority belongs in `agentic-harness`, not this implementation.

A shared invocation cache supplies deterministic, repository-local ignore-aware inventories. Product metrics exclude generated/vendor/test/tooling content. Symlink entries are skipped; explicit context reads are confined and bounded. Coverage reports expose skips and errors. Git-global ignores are intentionally disabled to keep results reproducible across machines.

Architecture analysis uses Oxc JavaScript/TypeScript/TSX syntax trees and extracts Vue/Svelte scripts. Runtime, type-only, and dynamic imports remain distinct. Iterative graph traversal finds synchronous runtime cycles. Resolution includes relative imports, emitted JS suffixes, declared JSON tsconfig paths, nearest-package aliases, and basic workspace exports. Unsupported resolution and parsing coverage are reported; see [command contracts](docs/cli-contracts.md).

Audit v2 represents unmeasured quality/readiness as null. Architecture exposes a versioned heuristic indicator with coverage and formula provenance. Gates validate artifacts before evaluating explicit policy; unknown/null metrics cannot satisfy numeric thresholds. Static design observations and agentic heuristics remain advisory and cannot establish production readiness.

The parser and AST visitor run in a reusable native child process launched from the same executable. Requests contain only bounded source text and its path; the worker does not load or execute project code. A parser crash (including stack exhaustion) becomes incomplete coverage, and the next file gets a fresh worker. Parent processes reap workers before normal CLI exit. The internal framed protocol is not a public API. Source reads remain capped at 2 MB; frames are capped at 16 MiB.
