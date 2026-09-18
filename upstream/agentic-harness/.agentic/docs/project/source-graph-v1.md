# Source Graph v1

Status: experimental canonical contract for issue #98. Schema: `catalog/schema/source-graph.v1.schema.json`.

## Purpose

Source Graph v1 is the language-neutral static-analysis boundary between source-language frontends and deterministic architecture checks. It records what a frontend actually observed and resolved; it is not a claim of compiler-grade semantic understanding.

## Frontend contract

A frontend owns language detection, syntax parsing, import/module extraction, language-specific package/workspace resolution, and explicit coverage. Core graph checks consume normalized nodes and edges and must not infer missing language semantics.

Each frontend declares an implementation/version and capability states (`supported`, `partial`, `unsupported`) for parsing, imports, packages, type edges, dynamic edges, workspace resolution, and framework extraction. A parser library is an implementation detail; Tree-sitter is permitted but not required.

Initial support sequence: JavaScript/TypeScript first, then Python, Rust and Go. Java/Kotlin, C#, PHP, Ruby, Swift and C/C++ follow measured demand and executable evidence.

## Edge semantics

- `runtime`: normal runtime dependency.
- `type`: compile/type-only dependency that must remain distinguishable from runtime cycles.
- `dynamic`: runtime dependency loaded dynamically or lazily.
- `development`: test/build/tool-only relation when deterministically known.
- `resource`: non-code resource reference.
- `package` / `module`: language-level package/module relation where a file-to-file target is not the correct abstraction.

`resolution` records whether the target is local, workspace-local, external, a resource, or unresolved. Unresolved edges never count as clean architecture evidence.

## Coverage

`complete=true` is permitted only when all discovered in-scope files were parsed and every dependency edge that requires resolution was resolved within the declared frontend capability. Consumers must still inspect the capability matrix: a complete syntax/import graph from a frontend that declares type edges unsupported is not complete type-semantic evidence.

Parser failures, unsupported syntax, excluded in-scope targets, ambiguous module resolution and unavailable workspace semantics remain visible through counters and `not_checked` entries. Policy may fail closed on incomplete coverage.

## Rule authority

Architecture rules must identify their scope separately from graph evidence:

- universal engineering rule;
- language rule;
- framework/ecosystem rule;
- project-selected rule.

Language conventions must not silently become universal requirements.

## Security boundary

Static frontends must not execute target project code. Optional compiler/build-tool integration requires explicit reviewed execution through the separate check-execution contract. A successful parse is not permission to run package managers, build scripts, plugins or project binaries.

## Compatibility

Consumers depend on this versioned graph shape and capability semantics, not parser-specific AST structures. New frontend capabilities may be added through a new compatible contract version; existing edge meanings must not change silently.