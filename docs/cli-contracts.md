# CLI contracts and compatibility

`ah --version` emits package and pinned source identities as JSON. Every command family is available through the installed `ah`. Compatibility binaries call the same library. JSON operations write results to stdout; invalid input uses a versioned `diagnostic` JSON object on stderr.

Exit 0 means the requested operation completed or explicit validation/policy passed. Exit 1 means a main audit/security finding, failed project validation or failed gate. Exit 2 means invalid input, unsupported functionality or execution failure. Architecture/design/agentic inspection returns 0 when it produces its report; inspect compliance, coverage and advisory findings rather than interpreting process success as a quality verdict. Help returns 0. Unknown flags, excess operands, missing values, invalid directories and unknown model IDs fail.

## Commands

| Surface | Behavior |
| --- | --- |
| `init TARGET`, `upgrade TARGET` | Compose current canonical files; repeated `--pack`, `--skill`, `--policy`; optional `--boilerplate` (`--template` alias), `--preset`, `--profile`, `--name`, `--maturity` |
| `validate [TARGET]`, `harness-audit [TARGET]` | Validate the selected project's YAML and routed context; no fallback to bundled templates |
| `catalog-check` | Validate embedded catalog materialization |
| `audit [TARGET]` | Codebase audit v2, with explicit unmeasured scores and discovered checks that have not been executed |
| `compare BEFORE AFTER` | Validate two matching-version audit artifacts; unknown deltas remain null |
| `gate AUDIT` | Validate artifact, reject explicitly incomplete evidence, then apply optional `--min-overall`, repeated `--min-score dimension=N`, `--max-architecture-errors`, `--fail-on-architecture-error` |
| `security-scan [TARGET]` | Bounded AWS access-key/private-key-marker baseline with redacted findings and skipped-file coverage |
| `architecture detect [TARGET]` | Framework/dependency and structural evidence, including nested package manifests |
| `architecture analyze [TARGET]` | Source graph and compliance; optional repeated `--profile` and `--as-of YYYY-MM-DD` |
| `architecture enforce [TARGET]` | Contract preview; `--write` saves it; supports profiles and an explicit date |
| `design-system-components [TARGET]` | Static capability suggestions; `--write` emits a review checklist |
| `design analyze [TARGET] --level static` | Design Analysis v1; optional `--output` |
| `design preserve --analysis FILE` | Review-required Design Genome candidate; optional `--output` |
| `design diff BEFORE AFTER` | Measured Design Analysis drift; optional `--output` |
| `design prompt --genome FILE --task FILE` | Compile only approved Design Genome input; optional `--output` |
| `agentic audit/context/skills/improve [TARGET]` | Experimental inspection/preview; no automatic application |
| `agentic models [TARGET] [--task NAME]` | Pinned model profiles with unknown compatibility/ranking represented as null |
| `agentic compare MODEL_A MODEL_B` | Inspect two known profiles |
| `agentic migrate [TARGET] --from MODEL_A --to MODEL_B` | Preview only; distinct from filesystem-layout migration |

## Evidence versions

The authoritative schemas are embedded from `agentic-harness/catalog/schema`. Codebase audit v2 and agentic inspection v2 distinguish static indicators from verified quality. `overall` and readiness remain null because the CLI has not executed release gates. A numeric threshold on a null metric fails with exit 1; an unknown dimension or invalid threshold fails with exit 2. A gate with no thresholds checks artifact validity and explicit completeness only.

Complete unversioned legacy audits remain readable under their original numeric-score contract. Comparisons cannot mix legacy and v2. Empty objects, bad finding/check shapes, invalid architecture counters, NaN, infinities and scores outside 0–100 are rejected. Date-bound exceptions remain active through the specified UTC calendar day; expired entries cannot waive violations.

## Scope and limits

Repository-local `.gitignore`, `.ignore`, and `.ahignore` apply to all scanners, including security scanning. Git-global/parent ignore files do not apply. Generated/vendor/build directories and symlink entries are excluded. A negation rule may restore ordinary locally ignored files; built-in generated-directory and symlink exclusions remain mandatory. Ignored files are outside the declared scope, so the secret scanner is not a whole-disk or whole-history scan. Scan errors and bounded-read skips are reported.

Runtime/type/dynamic import edges are separate; only synchronous runtime edges create cycle/boundary violations. Basic JSON tsconfig paths, exact/wildcard workspace exports and non-code resource imports are supported. JSONC, tsconfig inheritance, Vite-only aliases, computed import targets, complex conditional exports, and non-JS/TS language graphs remain unsupported coverage.

Oxc 0.139.0 parses JavaScript/TypeScript/TSX, including import-type generics, type re-exports, CommonJS import-equals and escaped specifiers. Parser dependencies are pinned together to retain Rust 1.94 compatibility. Static template literals are supported for dynamic imports. Source-phase/deferred imports remain explicit coverage gaps; bare `require` recognition is syntactic and does not resolve shadowed bindings. Type classification uses explicit source annotations, not compiler-dependent import elision. The native parser worker isolates stack exhaustion from report generation. Parser diagnostics or worker failure report incomplete coverage and withhold a score; these reports are not a TypeScript compilation verdict.

The architecture indicator is `100 - min(100, deterministic_errors * 100 / supported_source_files)`, a heuristic rather than a calibrated health score. It is unavailable without a supported complete graph.

Optional `.agentic/design-system.json` declares roots, required_components, exceptions and capability aliases. Shared native controls are expected implementation detail; controls and literal CSS colors outside shared roots are review observations. Type parameters, comments, scripts and tests are not template controls. Static observations do not establish visual, accessibility or runtime conformance. CSS-in-JS and full semantic component equivalence remain unmeasured.

## Project upgrades and recovery

New projects contain root `AGENTS.md` plus routed `.agentic` context and `.agentic/lock.json`. DESIGN and REFERENCE routes may be null. Upgrades preserve existing project/module files, report conflicting content, and merge requested module declarations into managed metadata. Checksum provenance distinguishes original installed bytes from customization. Back up an existing project before upgrading; retain the report and reconcile conflicts explicitly.

Legacy `agentic.yaml` remains readable for inspection. Upgrade refuses implicit layout migration and mixed manifests. Follow the canonical [explicit migration procedure](https://github.com/powerpuff-kitty/agentic-harness/blob/main/.agentic/docs/project/migration-v1.md): make a backup, map accepted context to `.agentic`, preserve custom routes, remove the legacy manifest only after reconciliation, and validate the result. There is no filesystem `ah migrate --apply` implementation in this release scope.
