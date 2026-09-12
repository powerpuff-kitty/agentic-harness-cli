# Experimental, non-executing check planning

`ah checks plan [TARGET] [--config PATH]` reads an explicitly authored policy and emits a JSON review preview. It does not discover, approve or run scripts. Exit 0 means the preview was produced; invalid or incomplete declared inputs exit 2 with a diagnostic. Existing audit/gate meanings are unchanged.

Create `.agentic/checks.json` in the target repository, using only input files/directories that exist and are appropriate to inspect:

```json
{
  "format_version": 1,
  "kind": "check-policy",
  "inputs": ["src", "package.json"],
  "checks": [{
    "id": "unit",
    "argv": ["npm", "test"],
    "cwd": ".",
    "required": true,
    "timeout_ms": 60000,
    "max_output_bytes": 65536
  }],
  "required_controls": [{"rule_id": "architecture.boundaries", "capability": "checked"}],
  "max_age_ms": 3600000
}
```

Run from the repository or pass its directory explicitly:

```sh
ah checks plan
ah checks plan ./project --config .agentic/checks.json
```

The JSON preview contains the interpreted policy, exact input manifest, source/policy/planner/review digests and source pins. Command strings are argument arrays, not shell lines. Executable identity, actual execution and host controls remain unverified. Required controls stay unverified even when their declarations exist. The review digest is not a token authorizing execution; `checks run`, `--apply`, `--approve` and `--write` are unsupported and rejected.

## Input and privacy boundaries

Inputs are explicit, normalized relative ASCII paths, not globs. `.` is allowed for cwd but not as an input root. Missing/unreadable paths, symlinks, non-regular files and exceeded limits fail rather than producing a successful partial snapshot. No implicit ignore rules hide declared files. Binary data and empty directories contribute to identity; overlapping roots are deduplicated. Paths containing `.git`, `.env` or `.env.*` are rejected, but this is not comprehensive secret detection. Do not select sensitive data under other names. Arguments and paths appear in the preview, so keep credentials out of policies and review output before sharing.

The exact snapshot intentionally differs from advisory code scanning: ignored files explicitly selected here remain inputs. Bounds are 2 MB/file, 64 MB total bytes, 10000 entries and depth 64; the policy file is capped at 262144 bytes. Unsupported names/scope must be revised explicitly, not silently skipped.

Two snapshots plus file metadata checks detect ordinary concurrent changes. This is a read-only tool for a non-hostile, quiescent worktree, not a sandbox against filesystem races. Source identity covers declared input bytes only, not external dependencies, host state or files omitted by the user. Executable resolution/environment review and before/after evidence are prerequisites for a future runner.

## Compatibility and evidence

Canonical contract: `upstream/agentic-harness/catalog/schema/checks.v1.schema.json`; semantics and length-framed SHA-256 algorithm: the pinned canonical `.agentic/docs/project/check-evidence-v1.md`. Digests bind exact bytes, so whitespace or line-ending changes can invalidate a preview. File hashes do not authenticate a producer or establish that tests ran.

`cargo test --locked --test checks` covers positive previews, no execution/writes, source/config changes, invalid fields, path escapes, scope limits and unsupported execution flags. Existing schema validation checks actual output, and the copied-candidate verifier exercises the planner without a real check executable on PATH. No workflow YAML changes are required.

Tracking: #55 and canonical #85. Approved execution, timeouts/process-tree cleanup, trusted evidence ingestion and freshness-aware completion gates remain unimplemented. The governance schema is a data contract, not evidence that an agent or host honored a policy.
