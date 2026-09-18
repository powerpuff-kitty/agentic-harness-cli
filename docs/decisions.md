# Decision Kernel runtime

The `ah decisions` family is the deterministic/offline runtime surface for the provider-neutral Decision Kernel contract pinned from `agentic-harness`.

The current slice intentionally does **not** call TypeSafe or any other hosted provider. It validates artifacts, fingerprints explicit JSON state and constructs a Jev request payload that can be inspected before any future authorized network transport is enabled.

## Commands

```bash
ah decisions validate decision.json
ah decisions fingerprint state.json
ah decisions jev-payload request.json specs.json --model jev-latest
```

### Validate

`decisions validate` checks the artifact kind and the v1 semantic invariants implemented by the CLI, then reports the exact embedded schema digest and canonical source pin.

Supported artifact kinds:

- `decision-spec`
- `decision-graph`
- `decision-request`
- `decision-provider-profile`
- `decision-policy`
- `decision-receipt`
- `decision-outcome`
- `decision-evaluation`

In addition to structural checks, the CLI validates invariants that are inconvenient to express completely in JSON Schema, including graph acyclicity, reducer references, evidence-coverage arithmetic, normalized probability distributions, provider consequence-authority prohibition and side-effect-free evaluation.

Passing validation means the artifact satisfies this supported contract. It does not establish factual correctness, provider quality, model calibration, source reliability or authorization to perform an action.

### Fingerprint

`decisions fingerprint` parses an explicit JSON state, serializes its semantic JSON value deterministically and returns a SHA-256 fingerprint.

The command reads only the file named by the user. It does not inspect repository context implicitly.

This first fingerprint profile is identified as `ah-json-sha256-v1`. Consumers should persist the algorithm identifier with the fingerprint so future canonicalization changes do not silently alter identity semantics.

### Jev payload

`decisions jev-payload` maps canonical atomic decision specs into the current TypeSafe Jev request shape:

| Decision Kernel | Jev |
| --- | --- |
| `boolean` | `noul` |
| `choice` | `choice` |
| `ordinal` | `score` |

Other Decision Kernel kinds are rejected by this adapter rather than guessed.

The payload contains only the explicit `decision-request.state.payload`, selected question specs and model name. It does not read repository context, credentials or environment variables. The output records `network_call_performed: false` and a redacted future authorization-header shape.

The default model name for payload construction is `jev-latest`; an explicit `--model` value can be supplied. This is payload construction only and does not establish that the model alias is reachable, billable, calibrated or suitable for a decision class.

## Security and authorization

- Decision providers have no consequence authority.
- Validation or provider output cannot authorize deployment, payment, deletion, trading or other consequential actions.
- Evaluation/shadow/counterfactual artifacts require no side effects.
- Provider confidence remains distinct from calibration, evidence coverage, evidence reliability, decision certainty and domain outcome probability.
- Secrets must come from runtime provider configuration when live transport is implemented; they must not be persisted in Decision Kernel artifacts.
- Missing, conflicting, out-of-distribution, abstained and provider-failure results remain explicit states.

## Runtime roadmap

This offline slice establishes the safe boundary required before live inference.

Follow-up under issue #77 adds opt-in hosted TypeSafe transport with:

- `TYPESAFE_API_KEY` read from the environment only;
- bounded timeout/retry/usage policy;
- stable diagnostics for credentials, quota, timeout and malformed responses;
- exact provider/model/usage provenance;
- deterministic response normalization into DecisionReceipt;
- no silent provider fallback;
- live tests opt-in only.

Calibration, threshold tuning, shadow/challenger evaluation, fan-out, cache/replay and adoption into automated routing remain separate work and must use the canonical contract rather than provider-specific application fields.
