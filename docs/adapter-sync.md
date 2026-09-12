# Context-only adapter installation

Experimental `ah adapters sync [TARGET] --host HOST [--profile PROFILE]` previews installation by default. `TARGET` defaults to the current directory, and must contain a nonempty UTF-8 `AGENTS.md`. Existing custom context routes remain untouched. The optional profile is `typed-ui`; `base` is the default.

```sh
ah adapters sync ./project --host claude
ah adapters sync ./project --host claude --profile typed-ui
```

Inspect the returned source revision, destination actions and hashes. To create the missing files, repeat the same selection with `--apply --review` and the exact returned `plan_digest`:

```sh
ah adapters sync ./project --host claude --apply --review sha256:REPLACE_WITH_REVIEWED_DIGEST
```

The placeholder is not executable approval; use the actual reviewed digest. A change to target, selection, router bytes, bundled inputs or destination contents invalidates the preview. The digest is a change guard, not a credential or an owner-authentication mechanism against another process running as the same user.

## Supported file scope

Claude base installs the pinned `CLAUDE.md` containing `@AGENTS.md`. The `typed-ui` profile additionally installs `.claude/rules/agentic-typed-ui.md`. Cursor base and Codex base are native-router no-ops; Cursor `typed-ui` installs `.cursor/rules/agentic-typed-ui.mdc`. Codex does not have a typed-UI asset profile here. When payloads are selected, `.agents/adapters/LICENSE` retains the authored MIT notice without replacing the application's root license.

Payloads are compiled from the exact agent repository pin. The runtime needs no network or sibling checkout. The CLI verifies the embedded source/destination inventory against the reviewed allowlist, rather than trusting filenames in the target repository. No settings, hooks, command scripts, permission files, root overrides or product decisions are generated.

## Preservation and failure behavior

Each destination is classified `create`, `unchanged` or `conflict`. Exact existing bytes are untouched. Different bytes are never automatically merged, replaced or deleted. A known conflict anywhere in the selected batch stops application before any writes, including the notice file. Reconcile conflicts manually and generate a fresh preview; there is no force flag.

Application stages complete file contents and uses `persist_noclobber` from the existing tempfile dependency. This never replaces an existing destination, but is not a portable multi-file atomic transaction. A failure can leave completed files and created directories; the `partial` report retains those paths so a fresh preview can resume. No automatic rollback deletes files that another actor may have changed. Crashes/interruption can also leave a temporary link, and directory durability is not guaranteed. Review an interrupted operation before retrying.

Target and destination symlinks/reparse points, non-directory parents, missing or invalid router text and oversized existing context files are rejected. Existing file reads are bounded at 65536 bytes. The filesystem checks assume a quiescent cooperative worktree; they do not defeat hostile concurrent path replacement. Installation does not parse or validate every reference in AGENTS.md, and does not verify which instructions a live host actually loads.

Exit 0: valid ready preview or fully applied/no-op batch. Exit 1: conflicts or partial application, with a structured report. Exit 2: invalid options/input or stale review, with a diagnostic. Old content is represented by hashes, not echoed in reports.

## Verification boundary

The report always keeps `host_delivery_verified` and `enforcement_verified` false. File installation is not a host session, model-performance evaluation, glob-engine verification or runtime policy enforcement. Exact-version host tests remain under the agents repository's issue #24.

Rust installation and refusal tests plus copied-candidate checks belong to CLI #65. Future managed replacement/removal needs separate preservation semantics and is not supported here. Draft check execution is unrelated and remains disabled.

Implementation API reference: https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html#method.persist_noclobber
