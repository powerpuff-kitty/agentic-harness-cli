# Experimental caller-approved completion

`ah checks complete` evaluates saved execution and governance evidence against current project inputs without running commands. It emits a separate verdict scoped to **declared checks and required controls**, under explicit caller trust. It does not authenticate producers or certify whole-project readiness.

First save a reviewed `checks run` report outside the declared input scope. Collect governance reports for every required rule/capability and the reference bytes supporting their claims. Author a manifest:

```json
{
  "format_version": 1,
  "kind": "check-evidence-manifest",
  "run": {"path": "evidence/run.json", "digest": "sha256:EXACT_RUN_BYTES"},
  "governance": [],
  "references": []
}
```

The digest above is a placeholder, not valid approval. Nonempty required governance controls need matching reports. A governance entry is `{ "path": "evidence/control.json", "digest": "sha256:EXACT_REPORT_BYTES", "producer": { "id": "reviewed-producer", "version": "reviewed-version" } }`. A reference entry is `{ "name": "opaque-reference-name", "path": "evidence/result.json", "digest": "sha256:EXACT_REFERENCE_BYTES" }`.

Review the report provenance, exact digests, producer identities, reference bindings, required policy and scope. Then supply the SHA-256 of the exact reviewed manifest bytes:

```sh
ah checks complete ./project --evidence evidence/manifest.json \
  --approve-evidence sha256:REVIEWED_MANIFEST_DIGEST
```

Never automatically pipe a calculated digest into this command for arbitrary project evidence. Approval is the caller's trust decision over those exact report/reference bindings, not a signature or an authorization to execute commands. No approval is persisted or read from repository settings. Signed producers are deferred.

Optional `--config` and `--settings` select the current project-relative policy/settings. The imported run must match the entire freshly prepared review: exact source/policy/settings, target, platform, executor bytes and native tools. Use the same CLI and target that produced the run; upgrading the binary invalidates old run evidence. Recompute check outcomes through the ledger and require exact planned order, argv/cwd and required flags. Required governance capabilities cannot substitute for one another.

Run start and governance observation times must be current within the selected policy age. Future, contradictory or stale timestamps fail. Source/config/tool changes after execution, missing results, supervisor faults, changed/unresolved references and duplicate/conflicting claims fail even if the imported summary says passed. Ordinary optional command failure follows the existing ledger behavior. Input/report bytes are rechecked during evaluation; no claim extends beyond its recorded evaluation time.

Paths are confined normalized project-relative paths without symlink/reparse-point traversal. No URLs are fetched. Keep evidence outside declared input roots to avoid digest self-reference. The canonical completion schema documents byte, count and timing limits; this is cooperative bounded I/O, not a hostile-race sandbox.

Exit 0 emits `check-completion`, `completion_verified:true`, `scope:declared-checks-and-required-controls`, `trust:caller-approved-exact-evidence`, and `producer_authenticated:false`. Exit 2 emits a diagnostic and no completion verdict. Existing audit/gate behavior is unchanged; the original execution report still has `completion_verified:false`.

This gate verifies integrity and semantic consistency under caller trust. It cannot prove that approved producers told the truth, that reference contents substantiate claims, or that a host actually enforced them. Clock trust, undeclared/transitive inputs, reverted mutations and changes after evaluation remain explicit limits. No imported command is executed and no artifact is implicitly written.

Validation: `scripts/validate-completion.py` runs authored synthetic positive/negative cases and validates actual output against pinned schemas. Candidate verification repeats these with a copied binary. Governance library tests cover semantic matrices; existing full Rust checks remain required. Windows lacks a verified executor and cannot produce a locally matching successful run; positive completion is not claimed there.

## Local verification — 2026-09-18

Implementation commit `1f08b82`; canonical contract pin `eac2d48`; agent-procedure pin `7e7d44e`. Tested debug binary SHA-256: `a5ab1faca2e0741496eb47049eefad5328c6d23c271f2d3496b70a01ea44f68c`.

On macOS x86_64, Rust 1.94.1: 190 Rust tests, Clippy with warnings denied and formatting passed. Actual contract/schema validation passed. Copied-binary candidate verification passed 25 completion cases covering required governance, explicit approval, altered metadata, cleanup integrity, stale/future/mismatched evidence, duplicate claims, changed references, path traversal, missing approval, producer mismatch and symlinks. Canonical contract/catalog and agent procedure validation passed. An old adapter test pin was corrected to assert the current lock; the skill-delivery evidence fixture was repinned without changing its file hashes.

The #86 reading-list trial passed unit/types/build/browser, accepted a saved run under the declared scope, then rejected it after a source change. That app trial has no required governance controls; the separate synthetic completion probes cover required governance. No actual producer signature or native coding-host/model execution is claimed. Source commits remain local/unpublished and remote platform CI is unobserved.
