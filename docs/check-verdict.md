# Experimental fail-closed outcome ledger

Tracking: CLI #61, part of #55. The ledger remains a pure Rust library component. The experimental
executor now integrates it; see `check-execution.md` for the separate process backend.

`agentic_harness_cli::check_verdict::RunLedger` keeps the reviewed check sequence,
its required flags and accepted observations. It does not spawn processes, read
files, approve commands, authenticate evidence or change existing artifact schemas.

## Decisions

A normal optional command failure may coexist with successful required checks.
A supervisor integrity failure may not. Execution errors, an unreaped spawned
child, missing/failed cleanup, missing capture evidence, contradictory exit status
or an unknown outcome stop the ledger even when that check is optional. A later
successful observation cannot clear the first stop reason.

The caller supplies required flags once through `CheckSpec` when creating the
ledger. Result metadata cannot downgrade a required check. Check IDs must be
unique and observations must match the reviewed order; duplicate, unknown or
extra results make the sequence unsuccessful.

Missing results are not silently accepted. `finish` preserves earlier observations
and records the remaining checks with `outcome: None` and a typed `skipped_because`
reason. Those entries describe absent execution; they are not invented process
results. A caller that encounters a later path/input validation error must call
`stop(HaltReason::InputRevalidation)` and finalize the ledger rather than discard
already-recorded outcomes. Budget exhaustion and supervisor failure have separate
stable reasons. The first reason is sticky while input invalidity is also retained.

## Trust and integration boundary

The ledger checks selected supervision invariants in an in-memory JSON value.
Full artifact-schema validation, reference verification, exact plan matching,
producer authentication and current-input/freshness evaluation remain separate.
A correctly shaped claim can still be false. The presence of a cleanup marker is
not independent proof that a process was actually cleaned up.

Records preserve original observations for their caller. They are not redacted
for publication: do not log or publish arbitrary input through this API. No raw
input appears in ledger error variants. `completion_verified` is always false.

The executor consults `may_continue` before dispatch, records every process outcome
before further fallible revalidation, and finalizes retained observations on later
path/input/deadline failures. Backend fault injection covers setup/read/signal/wait
failures and passes their outcomes through this ledger. The verified macOS
`no-live-group-members` cleanup state is accepted alongside a successful group
signal; arbitrary permission errors remain failures.

These integration tests supplement the pure ledger tests; neither authenticates
imported evidence or changes `completion_verified` from false.

## Validation

```sh
cargo test --locked --test check_verdict
```

The suite includes the original optional-cleanup counterexample, complete and
missing results, invalid metadata, contradictory exit/capture state, sticky halts,
retained partial results and an exhaustive status/reaping/cleanup fault matrix.
All tests use synthetic in-memory observations, not child processes. Existing
platform CI discovers this integration suite without workflow YAML changes.
