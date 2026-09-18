# Decision Kernel runtime

The `ah decisions` family is the provider-neutral Decision Kernel runtime pinned from `agentic-harness`.

All contract, planning, replay, outcome and payload commands remain deterministic/offline. Hosted TypeSafe evaluation is a separate opt-in path: `jev-evaluate` performs network I/O only when the caller explicitly supplies `--allow-network` and configures `TYPESAFE_API_KEY` in the process environment.

## Commands

```bash
ah decisions validate decision.json
ah decisions fingerprint state.json
ah decisions plan graph.json specs.json state.json --provider typesafe-jev --mode shadow
ah decisions replay graph.json receipts.json
ah decisions outcome receipt.json --id outcome-1 --observed-at 2026-09-19T10:00:00Z --label confirmed --verification human
ah decisions compare-receipts champion.json candidate.json --mode shadow --dataset triage-v1 --revision 1 --generated-at 2026-09-19T10:05:00Z --changed provider
ah decisions calibration-report calibration-dataset.json --generated-at 2026-09-19T10:10:00Z --target-accuracy 0.95 --min-coverage 0.70 --min-samples 30
ah decisions calibration-compare baseline.json candidate.json --generated-at 2026-09-19T10:15:00Z --max-accuracy-drop 0.01 --max-coverage-drop 0.02
ah decisions jev-payload request.json specs.json --model jev-latest
ah decisions jev-evaluate request.json specs.json --allow-network --decided-at 2026-09-18T19:30:00Z --model jev-latest --timeout-ms 10000 --max-retries 2
ah decisions jev-receipts request.json specs.json response.json --decided-at 2026-09-18T19:30:00Z --evidence evidence.json
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

### Plan and fan-out

`decisions plan` validates a DecisionGraph and registry of DecisionSpecs against one explicit state snapshot, then emits topological stages. Nodes in the same stage are independent and may fan out in parallel; dependent nodes appear in later stages.

Every node also receives a stable cache identity derived from the state fingerprint, spec ID/revision, provider hint and execution mode. Changing any of those inputs invalidates the identity rather than silently reusing stale inference.

Planning performs no provider call and no side effect.

### Replay

`decisions replay` consumes recorded DecisionReceipts and a DecisionGraph. It rebuilds node inputs and deterministic reducer input references without re-inference.

Replay refuses receipts that mix state fingerprints and reports unresolved nodes explicitly. It does not execute application/domain reducers because their implementation belongs to the consuming project.

### Outcomes

`decisions outcome` creates an immutable `DecisionOutcome v1` linked to a prior receipt. It records an observed label, verification type, optional verification/action references and whether the observation is usable for evaluation.

The original DecisionReceipt is read and validated but never modified. Outcomes therefore append later knowledge instead of rewriting what the system knew at decision time.

### Shadow, champion/challenger and counterfactual comparison

`decisions compare-receipts` compares two valid receipts and emits a canonical `DecisionEvaluation v1` artifact.

Supported modes:

- `shadow`
- `champion-challenger`
- `counterfactual`

The comparison records result/disposition agreement and provider-confidence delta when both receipts expose confidence. It always emits `side_effects: false` and performs no provider call or authorization.

Counterfactual comparison requires explicit `--changed` dimensions such as `provider`, `model`, `policy`, `threshold`, `evidence`, `spec`, or `state`. Different state/spec identities are rejected unless the corresponding change dimension is declared.

These artifacts are engineering evidence, not model-quality proof by themselves. Representative datasets, outcome labels and calibration remain separate evaluation work.

### Empirical calibration

`decisions calibration-report` evaluates a versioned `DecisionEvalDataset v1` entirely offline.

Each dataset case contains the immutable DecisionReceipt plus independently verified expected truth. The dataset itself fixes:

- train/calibration/validation/test split;
- DecisionSpec ID/revision and decision kind;
- state schema/version;
- one exact provider/model/version identity.

The evaluator reports:

- produced coverage;
- accuracy among produced decisions;
- abstention and provider-failure rates;
- Brier score and log loss when result distributions exist;
- expected calibration error and equal-width reliability bins when provider confidence exists;
- ordinal mean absolute error for ordered decisions;
- mean latency and total cost only when those fields are complete across the dataset.

Missing quantities remain `null`. Missing provider confidence is never treated as zero and missing cost/latency is never estimated.

Threshold fitting is opt-in with `--target-accuracy` and `--min-coverage`, and is permitted **only** when the dataset split is `calibration`. The selected threshold is the lowest observed provider-confidence value that satisfies target accuracy, minimum coverage and `--min-samples`. Test/validation splits can be measured but cannot fit a threshold.

A threshold remains policy evidence only. The report always records `side_effects: false` and `consequence_authorized: false`.

### Calibration regression gate

`decisions calibration-compare` compares two `DecisionCalibration v1` reports produced on the exact same dataset/spec/state-schema identity. No provider call is made.

By default the command permits no regression in accuracy, coverage, Brier score, ECE or ordinal MAE. Explicit budgets can relax those limits:

- `--max-accuracy-drop`
- `--max-coverage-drop`
- `--max-brier-increase`
- `--max-ece-increase`
- `--max-ordinal-mae-increase`
- optional `--max-latency-increase-ms`
- optional `--max-cost-increase-usd`

A passing report exits 0. A quality regression emits the complete machine-readable `DecisionRegression v1` report and exits 1, making it suitable for CI without contacting a hosted provider. Invalid/mismatched reports exit 2.

If a metric is unavailable for both baseline and candidate it is treated as not applicable. If a budgeted metric exists on one side but is missing on the other, the gate fails closed for that metric.

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


### Hosted Jev evaluation

`decisions jev-evaluate` is the only current Decision Kernel command that performs hosted network I/O.

It requires all of the following:

- explicit `--allow-network`;
- `TYPESAFE_API_KEY` from the environment only;
- an explicit `--decided-at` timestamp so resulting receipts do not invent decision time;
- the same explicit DecisionRequest and DecisionSpec files accepted by `jev-payload`.

The transport is deliberately narrow:

- endpoint is pinned to `https://api.typesafe.ai/v1/systemone`;
- HTTPS is mandatory;
- redirects are disabled so the bearer credential cannot be forwarded elsewhere;
- environment HTTP(S) proxy routing is disabled for the secret-bearing request;
- only the explicit `decision-request.state.payload` and selected reviewed question specs are sent;
- no repository files, git metadata, memory, agent context or unrelated environment variables are collected;
- API keys are never printed or persisted; diagnostics show only `Bearer <redacted>`.

Defaults follow the current TypeSafe client contract: `jev-latest`, 10,000 ms per attempt and two retries after the initial attempt. CLI overrides are bounded to at most 60,000 ms per attempt, five retries and a 120,000 ms aggregate transport budget.

Retries are limited to transport failures and retryable HTTP conditions (408, 429 and 5xx). Server `retry-after-ms` or numeric `Retry-After` values are honored only up to 60 seconds; otherwise deterministic exponential backoff starts at 500 ms and caps at 5 seconds.

Successful live responses are validated and immediately normalized into the same review-required DecisionReceipt set produced by `jev-receipts`. The transport envelope records provider/model usage, attempt count, elapsed time and `cost_usd: null` when the API does not return a measurable cost. Provider confidence remains uncalibrated/unknown for project policy until representative evaluation evidence exists.

Stable hosted-provider diagnostics include:

| Code | Meaning |
| --- | --- |
| `provider-network-disabled` | `--allow-network` was not supplied |
| `provider-credentials-missing` | `TYPESAFE_API_KEY` is absent |
| `provider-credentials-invalid` | API key is empty/malformed |
| `provider-timeout-invalid` / `provider-retries-invalid` / `provider-budget-invalid` | Local transport policy is invalid |
| `provider-authentication` | HTTP 401 |
| `provider-request-rejected` | HTTP 422 |
| `provider-rate-limited` / `provider-quota` | rate/quota/billing refusal |
| `provider-overloaded` | HTTP 529 |
| `provider-server-error` | other retry-exhausted 5xx |
| `provider-timeout` / `provider-connection` / `provider-tls` / `provider-transport` | bounded transport failure |
| `provider-malformed-response` | success body is invalid or violates the Jev response contract |

Hosted provider failures exit separately from ordinary invalid CLI input and never trigger another model/provider automatically.

### Jev response normalization

`decisions jev-receipts` converts an already-recorded TypeSafe response into canonical DecisionReceipts.

It validates answer variants against their DecisionSpecs:

- Noul becomes a boolean result plus the explicit yes/no distribution; two-sided certainty is `max(p, 1-p)`.
- Choice must select one declared option and return exactly the declared option distribution.
- Score must return a legend matching the declared ordered levels.

The command never interprets vendor confidence as domain outcome probability. Calibration remains `unknown` until the project has representative empirical evidence. New receipts are review-required with the `unapplied` policy marker: normalization is not policy acceptance.

Required evidence is resolved only from the explicit optional evidence manifest supplied by the caller. Missing requirements lower evidence coverage rather than being inferred from arbitrary state fields.


## Security and authorization

- Decision providers have no consequence authority.
- Validation or provider output cannot authorize deployment, payment, deletion, trading or other consequential actions.
- Evaluation/shadow/counterfactual artifacts require no side effects.
- Provider confidence remains distinct from calibration, evidence coverage, evidence reliability, decision certainty and domain outcome probability.
- Secrets must come from runtime provider configuration when live transport is implemented; they must not be persisted in Decision Kernel artifacts.
- Missing, conflicting, out-of-distribution, abstained and provider-failure results remain explicit states.

## Runtime roadmap

The shared runtime now covers offline validation/fingerprinting, fan-out planning, stable cache identities, replay, outcome feedback, shadow/challenger/counterfactual evidence, Jev payload construction, recorded-response normalization and opt-in hosted Jev transport.

Remaining work under the Decision Kernel roadmap is primarily empirical and product-specific: representative calibration datasets, threshold tuning, production cache persistence, durable workflow integration and adoption into application routing only where evaluation demonstrates acceptable behavior.
