# Decision Kernel architecture

The Decision Kernel contract defines how uncertain judgments enter ordinary software without giving a model ownership of policy, authorization, workflow, or side effects.

It is provider-neutral. TypeSafe Jev is one possible semantic provider; deterministic rules, statistical models, language models and humans can implement the same decision boundary when their capabilities fit the DecisionSpec.

## Core rule

> Code calculates. Decision providers judge. Policy authorizes. Workflows coordinate. Humans remain explicit providers/reviewers where required.

A provider result is evidence for application policy. It is not itself permission to perform a consequential action.

## Contract flow

```text
Domain data / evidence
        |
        v
immutable state snapshot
        |
        v
DecisionSpec + DecisionGraph
        |
        v
DecisionRequest
        |
        +--> deterministic provider
        +--> Jev / semantic provider
        +--> statistical / ML provider
        +--> human provider
        |
        v
DecisionReceipt
        |
        v
DecisionPolicy
        |
        +--> accepted advisory result
        +--> review
        +--> abstain
        +--> reject
        |
        v
separately authorized workflow/action
        |
        v
DecisionOutcome
        |
        v
DecisionEvaluation / calibration / replay
```

## Public v1 contracts

- `decision-spec.v1.schema.json`: one atomic, versioned decision definition.
- `decision-graph.v1.schema.json`: dependency graph for composable decisions; reducers are deterministic.
- `decision-request.v1.schema.json`: immutable state identity, requested specs, execution mode and bounded budget.
- `decision-provider-profile.v1.schema.json`: provider capabilities and confidence semantics. Providers have `consequence_authority: false`.
- `decision-policy.v1.schema.json`: evidence, uncertainty, review and invariant policy. Consequential authorization remains separate.
- `decision-receipt.v1.schema.json`: immutable result provenance, evidence coverage and uncertainty dimensions.
- `decision-outcome.v1.schema.json`: later observed outcome/verification used as feedback without rewriting the original receipt.
- `decision-evaluation.v1.schema.json`: offline, replay, shadow, champion/challenger and counterfactual evaluation. Evaluation side effects are forbidden.

These contracts allow additive compatible fields within v1. Semantic validators must additionally enforce graph referential integrity, acyclicity and arithmetic invariants that JSON Schema cannot prove conveniently.

## Decision primitives

v1 recognizes:

- `boolean`
- `choice`
- `ordinal`
- `ranking`
- `estimate`
- `distribution`
- `extraction`
- `constraint`
- `optimization`

A provider must advertise which kinds it supports. This list describes the decision contract, not a requirement that every provider implement every primitive. Jev-style Noul/Choice/Score map naturally to boolean/choice/ordinal.

Prefer deterministic calculation for mechanically knowable values. An `estimate`, `distribution`, `constraint` or `optimization` decision should normally be produced by mathematical/statistical code unless a project explicitly documents a different provider and evaluation basis.

## Evidence coverage

A DecisionSpec declares evidence requirements. A DecisionReceipt records:

- evidence actually used;
- required evidence that is missing;
- required-present / required-total counts;
- a normalized evidence-coverage value.

High provider confidence does not compensate for missing required evidence. Policy may force review, abstention or rejection when evidence coverage is insufficient.

## Uncertainty is multidimensional

Do not collapse uncertainty into one `confidence` number.

The receipt separates:

1. **provider confidence** — whatever confidence/probability the producer supplies;
2. **calibration status** — whether that value has empirical calibration evidence;
3. **evidence coverage** — how much required evidence is present;
4. **evidence reliability** — an optional separately derived reliability assessment;
5. **decision certainty** — optional application-level certainty derived under a documented method.

An outcome probability is a different domain quantity. The receipt schema explicitly rejects an `outcome_probability` field so semantic confidence cannot silently become, for example, a market probability.

## Abstention and non-success states

The contract distinguishes:

- `produced`
- `unknown`
- `insufficient-evidence`
- `conflicting-evidence`
- `out-of-distribution`
- `abstained`
- `provider-failure`

Consumers must preserve these states. They must not coerce them to false, zero, success or an accepted decision.

## Decision graphs and deterministic reducers

DecisionGraph captures dependencies between atomic decisions. It is intentionally not a general workflow engine.

Independent nodes may execute in parallel. Dependent nodes wait for their declared inputs. Composition/reduction is deterministic application code so the provider does not silently change business weighting or control flow.

Runtime implementations must reject duplicate node IDs, missing dependencies and cycles.

## Policy and invariants

DecisionPolicy is evaluated outside the provider. It combines risk, evidence coverage, calibrated/uncalibrated confidence, failure states and review requirements into a disposition.

Invariants are named, reviewable and deterministically enforced. Examples:

- a model cannot promote unverified evidence to verified;
- semantic confidence cannot authorize a payment, deployment, transaction or trade;
- missing required evidence cannot become a positive verified conclusion;
- a provider result cannot execute a side effect.

High-consequence systems should use `separate-authorization-required`, even when a semantic decision is accepted as advisory input.

## Human provider

Human judgments should use the same state/spec/receipt boundary when practical. A human provider does not make the model authoritative; it makes provenance and review history comparable.

Professional or authoritative verification remains distinct from a model result and may be represented later as DecisionOutcome verification.

## Outcomes and feedback

A DecisionReceipt is immutable historical evidence. Later facts are appended as DecisionOutcome:

```text
state -> decision -> policy -> action -> observed outcome -> evaluation
```

This supports labeled eval datasets, calibration and quality measurement without rewriting what the system knew at decision time.

## Shadow, challenger and counterfactual evaluation

DecisionEvaluation supports:

- **offline**: evaluate a provider/spec against a fixed dataset;
- **replay**: re-run deterministic composition against recorded historical inputs;
- **shadow**: compute a candidate alongside production without affecting behavior;
- **champion-challenger**: compare production and candidate variants on the same evidence;
- **counterfactual**: change declared dimensions such as provider, policy or threshold and compare results.

All evaluation artifacts require `side_effects: false`. Switching a provider or policy therefore remains an explicit engineering/release decision.

## Temporal correctness

State fingerprints identify immutable snapshots. Domain systems that need historical correctness should additionally model valid/effective time separately from observation/ingestion time.

A replay must resolve the exact historical state and provider/spec/policy identity. It must fail explicitly rather than silently substituting current evidence.

## Runtime boundary

This repository owns the canonical schemas and architectural semantics.

`agentic-harness-cli` owns deterministic validation, provider mechanics, replay/cache/fan-out and executable policy checks. `agentic-harness-agents` owns reusable agent guidance. Neither may silently redefine the contracts here.

The canonical repository does not claim that publishing these schemas makes provider execution, calibration or host enforcement operational.
