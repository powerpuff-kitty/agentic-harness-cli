# Experimental, non-executing check planning

`ah checks plan [TARGET] [--config PATH]` reads an explicitly authored policy and emits a JSON review preview. It does not discover, approve or run scripts. Exit 0 means the preview was produced; invalid or incomplete declared inputs exit 2 with a diagnostic. Existing audit/gate meanings are unchanged.

Create `.agentic/checks.json` in the target repository using only input files/directories that exist and are appropriate to inspect:

```json
{
  "format_version": 1,
  "kind": "check-policy",
  "inputs": ["src", "package.json"],
  "checks": [{
    "id": "unit",
    "argv": ["node", "--test"],
    "cwd": ".",
    "required": true,
    "timeout_ms": 60000,
    "max_output_bytes": 65536
  }],
  "required_controls": [{"rule_id": "architecture.boundaries", "capability": "checked"}],
  "max_age_ms": 3600000
}
```

```sh
ah checks plan
ah checks plan ./project --config .agentic/checks.json
```

The preview contains the interpreted policy, exact input manifest, source/policy/planner/review digests and source pins. Executable identity, actual execution and host controls remain unverified. The plan review digest is not an execution approval token. `--apply`, `--approve` and `--write` are unsupported. Duplicate object keys in policy JSON are rejected rather than silently taking the last value.

## Separate execution review

A separate [opt-in execution workflow](check-execution.md) now uses `checks prepare` to review absolute tool bindings and the explicit environment, then requires its own matching approval digest plus `--allow-unsandboxed` for `checks run`. That does not change `checks plan`: it stays non-executing. Linux/macOS support local execution; Windows explicitly refuses execution pending equivalent cleanup support. No workflow here establishes host sandboxing or owner authentication.

## Input and privacy boundaries

Inputs are explicit normalized relative ASCII paths, not globs. `.` is allowed for cwd but not as an input root. Missing/unreadable paths, symlinks/reparse points, non-regular files and exceeded limits fail rather than producing a successful partial snapshot. No implicit ignore rules hide declared files. Binary data and empty directories contribute to identity; overlapping roots are deduplicated. Paths containing `.git`, `.env` or `.env.*` are rejected, but this is not comprehensive secret detection. Do not select sensitive data under other names. Arguments and paths appear in previews, so keep credentials out of policies.

Bounds: 2 MB/file, 64 MB total, 10000 entries, depth 64 and 262144 policy bytes. Unsupported scope must be revised explicitly, not silently skipped. Two snapshots plus metadata checks detect ordinary concurrent changes in a quiescent non-hostile worktree, not every malicious race. Identity covers declared input bytes, not host state, transitive dependencies or omitted files.

## Compatibility

Canonical contract: `upstream/agentic-harness/catalog/schema/checks.v1.schema.json`. Digest framing and semantics are in its accompanying `check-evidence-v1.md`. Exact-byte hashing means whitespace or line-ending changes invalidate a preview. Hashes do not authenticate a producer or prove tests ran.

Planning tests cover non-execution, deterministic output, byte changes, malformed inputs, bounds and platform path restrictions. Actual output and independent hash framing are checked against pinned schemas. Trusted imported-evidence evaluation, freshness after execution and whole-project completion remain separate unfinished work under CLI #55 and canonical #85.
