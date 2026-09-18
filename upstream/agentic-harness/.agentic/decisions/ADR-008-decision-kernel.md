# ADR-008: Adopt a provider-neutral Decision Kernel contract

- Status: accepted
- Date: 2026-09-18

## Context

Agentic systems increasingly need small uncertain judgments for routing, classification, evidence assessment and review gates. Treating every judgment as an unconstrained agent or prompt makes control flow hard to reproduce, mixes confidence with truth, and risks allowing a model output to become policy or authorization.

TypeSafe Jev provides useful typed semantic primitives, but project architecture must not depend on one hosted provider. Existing software disciplines already separate deterministic decisions, policy, workflow, provenance, model evaluation and human review.

## Decision

Adopt a provider-neutral Decision Kernel contract under `catalog/schema/`.

The v1 contract separates:

1. immutable state identity and evidence;
2. atomic versioned DecisionSpec definitions;
3. DecisionGraph dependencies with deterministic reducers;
4. provider capability/profile and bounded DecisionRequest;
5. immutable DecisionReceipt provenance and uncertainty;
6. separately evaluated DecisionPolicy and deterministic invariants;
7. later DecisionOutcome feedback;
8. side-effect-free replay, shadow, champion/challenger and counterfactual DecisionEvaluation.

Providers, including Jev, deterministic rules, LLMs, ML models and humans, have no consequence authority in the provider contract. A result may inform policy but cannot itself authorize a destructive or high-consequence action.

Provider confidence, calibration status, evidence coverage, evidence reliability, decision certainty and domain outcome probability remain distinct concepts. Non-success states such as insufficient evidence, conflict, out-of-distribution, abstention and provider failure are first-class.

## Consequences

- The canonical repository owns schemas and semantics; the CLI owns executable validation/provider mechanics and agents own procedures.
- Jev can be integrated or replaced without changing domain state models.
- Projects can record reproducible decision receipts and outcomes and can benchmark providers safely in shadow/challenger mode.
- JSON Schema alone cannot establish graph acyclicity, evidence arithmetic, calibration quality or factual correctness; semantic validators and empirical evals remain required.
- Decision graphs do not replace durable workflow engines, policy engines, deterministic calculators or domain-specific authorization.
- High-consequence consumers must define separate authorization and human/authoritative verification where appropriate.
