# Experimental opt-in local check execution

`ah checks prepare` reviews explicit executable bindings and environment settings without running anything. `ah checks run` can execute those reviewed commands on Linux/macOS only after a matching review digest and an explicit unsandboxed acknowledgment. Windows review works, but execution reports unsupported and launches nothing.

This backend is for trusted local commands and cooperative repositories. It does not isolate filesystem/network access or contain a malicious program. Use a separately sandboxed host for untrusted code. An agent with unrestricted shell access can supply flags itself; these flags are not owner authentication or a replacement for host approval controls.

## Configure and inspect

First author `.agentic/checks.json` as described in [check planning](check-planning.md), including every relevant source/config input. Then author `.agentic/check-execution.json` with actual absolute paths from your environment:

```json
{
  "format_version": 1,
  "kind": "check-execution-settings",
  "tools": {"node": "/absolute/path/to/node"},
  "environment": {"PATH": "", "CI": "1"},
  "max_total_ms": 60000
}
```

The tool map must exactly cover the executable names used as the first elements of policy argv arrays. Paths must point to native executables; for script launchers, bind the native interpreter and explicitly include the script in argv and appropriate input scope. Tool paths may resolve through symlinks; the requested path, resolved path, file mode and exact bytes are fingerprinted and rechecked. Runtime versions stay unverified; preparation never implicitly runs `--version`.

The child environment is cleared, then populated only from this explicit object. PATH is required: empty means no search path; nonempty entries must be absolute. Declaring PATH does not fingerprint every downstream executable. Policy/settings duplicate keys, including escaped aliases, are rejected. Never put secrets in arguments or settings: these values are present in local reviews/reports.

```sh
ah checks prepare ./project
```

Inspect the commands, input scope, tool paths/hashes, explicit environment, platform, limits and `approval_digest`. After a real review, supply that exact digest:

```sh
ah checks run ./project --approve-review sha256:REVIEWED_DIGEST --allow-unsandboxed
```

`REVIEWED_DIGEST` is a placeholder, not a usable approval. Do not automatically pipe a fresh digest into the run command or let generated repository text grant itself approval. No approval is persisted. `--config` and `--settings` accept project-relative overrides; repeated/unknown flags fail.

## Results and limits

Execution is sequential, without an implicit shell or inherited stdin. The Linux/macOS supervisor starts a process group, polls nonblocking pipes, enforces requested time/output limits, stops ordinary same-group descendants and reaps the direct child. Captured output is bounded across both streams, with one additional byte permitted for detecting overflow. Cleanup uses a 250 ms polling grace window, followed by up to 250 ms of exceptional direct-child recovery. If recovery cannot collect status, a non-signaling waiter retains the child while the report remains an integrity error; process termination can still end that waiter. Deliberately detached descendants can escape; process groups are not a sandbox. OS calls and scheduling are not real-time guarantees.

Source/policy/settings/tool identity is checked again before each command and after execution. A detected change stops subsequent commands and prevents passing results. No rollback of arbitrary command side effects is attempted. Checks should not change declared inputs. Files outside declared scope and changes reverted between snapshots remain outside this evidence model.

Reports distinguish passed, failed, timeout, output-limit, execution-error, skipped and unsupported. They include timestamps, duration, exit/signal status and direct-child cleanup observations. Raw logs are omitted; hashes and observed byte counts distinguish complete output from observed prefixes. No report/log file is implicitly written. Keep redirected reports outside declared inputs and review local paths, argv and hashes before sharing.

Exit 0 from `checks run` means the required local commands passed and reviewed inputs remained current. Exit 1 means failed/unsupported/invalidated results; exit 2 means invalid settings or approval before execution. Ordinary optional command failures remain visible. Supervisor, capture or cleanup failures halt dispatch even for optional checks; later cwd/input errors preserve earlier observations through the outcome ledger. `completion_verified` stays false and governance requirements stay unverified: local command success is not a project readiness badge or proof of host enforcement.

The existing `checks plan` remains read-only and never grants execution. Existing audit/gate semantics do not change. The separate caller-approved `checks complete` command evaluates imported evidence and freshness under explicit trust. Signed producers, Windows execution support and measured tool-version/transitive identity handling remain open under catalog #85 / CLI #55.

## Verification

`cargo test --locked --test check_execution` uses only disposable native test helpers. Candidate/schema checks use explicitly authored synthetic Python programs, never a user's real policy. Tests cover success, failure, timeout, output overflow, environment isolation, stale reviews, changed inputs and ordinary descendant cleanup. Windows tests verify review/refusal, not execution support. Existing validation entrypoints are reused; no workflow YAML change is needed.

Canonical artifact schema and full semantics are in the pinned catalog's `check-execution.v1.schema.json` and `.agentic/docs/project/check-execution-v1.md`. Resolve references from pinned local schemas, not over the network.

## Invocation budgets and process ownership

`max_total_ms` covers entry into `checks run` through initial review, all checks and final revalidation. Preparation uses the same selected bound for its own review. All file/tool read loops check the monotonic deadline before and after reads; OS I/O and scheduling cannot be forcibly bounded by this cooperative implementation. Cleanup grace may exceed the invocation deadline, but late completion/revalidation cannot turn into success.

Tool review is capped at 256 MiB per native file and 512 MiB cumulatively per review, including the executor. Canonical aliases share a fingerprint only within one review; each revalidation re-reads identities. Reports expose optional run timing, per-outcome execution/cleanup/recovery timing and a sticky `halt_reason`. The code uses the existing pinned libc 0.2.189 package as a direct platform dependency for verified ABI definitions.

The supervisor observes terminal status with `waitid(WNOWAIT)`, retains the unreaped leader through the final group signal and only then collects status. Lost ownership (`ECHILD`) forbids group signaling. On macOS, group signaling can return EPERM for a zombie-only group; bounded `proc_listpgrppids`/`proc_pidinfo` inspection may establish the distinct `no-live-group-members` cleanup state. Any live member, inspection failure or truncated list remains a cleanup failure. This new enum requires the updated canonical source pin; older strict consumers must upgrade.

SIGINT/SIGTERM set an atomic cancellation flag, then ordinary supervisor code stops the owned group and collects the direct child. SIGKILL, crashes, detached descendants and uninterruptible OS behavior remain outside these guarantees. Tests never attempt PID reuse or signal unrelated processes.

See [executor correction evidence](check-execution-verification.md) for measured local results. Cross-platform CI, release-candidate provenance and authenticated imported completion evidence are separate gates.

The [governance evidence evaluator](governance-verdict.md) now supplies pure semantic rejection of stale, mismatched and conflicting imported claims under explicit caller trust. The separate [caller-approved completion command](check-completion.md) combines it with current snapshots and saved run validation. Signed producer authentication remains deferred.
