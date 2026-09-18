#!/usr/bin/env python3
"""Validate Decision Kernel v1 contracts, fixtures and semantic invariants."""
from __future__ import annotations

import copy
import json
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[2]
SCHEMA_DIR = ROOT / "catalog" / "schema"

SCHEMAS = {
    name: Draft202012Validator(json.loads((SCHEMA_DIR / name).read_text()))
    for name in [
        "decision-spec.v1.schema.json",
        "decision-graph.v1.schema.json",
        "decision-request.v1.schema.json",
        "decision-provider-profile.v1.schema.json",
        "decision-policy.v1.schema.json",
        "decision-receipt.v1.schema.json",
        "decision-outcome.v1.schema.json",
        "decision-evaluation.v1.schema.json",
    ]
}


def validate(name: str, value: dict) -> None:
    SCHEMAS[name].validate(value)


def graph_semantic_errors(value: dict) -> list[str]:
    errors: list[str] = []
    ids = [node["id"] for node in value["nodes"]]
    if len(ids) != len(set(ids)):
        errors.append("duplicate decision node id")
    known = set(ids)
    deps = {node["id"]: list(node["depends_on"]) for node in value["nodes"]}
    for node_id, node_deps in deps.items():
        for dep in node_deps:
            if dep not in known:
                errors.append(f"unknown dependency {dep} from {node_id}")
            if dep == node_id:
                errors.append(f"self dependency {node_id}")

    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(node_id: str) -> None:
        if node_id in visiting:
            errors.append(f"cycle detected at {node_id}")
            return
        if node_id in visited:
            return
        visiting.add(node_id)
        for dep in deps.get(node_id, []):
            if dep in known:
                visit(dep)
        visiting.remove(node_id)
        visited.add(node_id)

    for node_id in ids:
        visit(node_id)

    reducer_ids = [item["id"] for item in value.get("reducers", [])]
    if len(reducer_ids) != len(set(reducer_ids)):
        errors.append("duplicate reducer id")
    for reducer in value.get("reducers", []):
        for source in reducer["inputs"]:
            if source not in known:
                errors.append(f"reducer input is not a graph node: {source}")
    return errors


def receipt_semantic_errors(value: dict) -> list[str]:
    errors: list[str] = []
    coverage = value["uncertainty"]["evidence_coverage"]
    present = coverage["required_present"]
    total = coverage["required_total"]
    expected = 1.0 if total == 0 else present / total
    if present > total:
        errors.append("required evidence present exceeds total")
    if abs(coverage["value"] - expected) > 1e-9:
        errors.append("evidence coverage arithmetic mismatch")
    if len(coverage["missing"]) != total - present:
        errors.append("evidence coverage missing-count mismatch")

    distribution = value.get("result", {}).get("distribution")
    if distribution:
        total_probability = sum(distribution.values())
        if abs(total_probability - 1.0) > 1e-6:
            errors.append("result distribution does not sum to 1")
    return errors


def evaluation_semantic_errors(value: dict) -> list[str]:
    coverage = value["coverage"]
    if coverage["evaluated"] + coverage["abstained"] + coverage["failed"] != coverage["total"]:
        return ["evaluation coverage arithmetic mismatch"]
    if value["mode"] == "counterfactual" and not value.get("changed_dimensions"):
        return ["counterfactual evaluation must declare changed_dimensions"]
    return []


spec = {
    "format_version": 1,
    "kind": "decision-spec",
    "id": "property.zoning-conflict",
    "revision": 4,
    "decision_kind": "boolean",
    "description": "Assess whether current evidence indicates a zoning conflict.",
    "input": {
        "schema_id": "lahaku.property-decision-state",
        "schema_version": 3,
        "immutable_snapshot_required": True,
    },
    "provider_capabilities": ["deterministic", "semantic", "human"],
    "evidence": {
        "requirements": [
            {"id": "zoning.sources", "required": True, "description": "Current zoning evidence."},
            {"id": "parcel.geometry", "required": True, "description": "Parcel geometry for evidence scope."},
        ]
    },
    "uncertainty": {
        "allow_abstain": True,
        "minimum_evidence_coverage": 1.0,
        "require_calibrated_confidence": False,
    },
    "policy": {
        "risk": "high",
        "consequential_action": "review-required",
        "human_review_required": True,
    },
    "evaluation": {"dataset_id": "zoning-conflict", "dataset_revision": 7},
}
validate("decision-spec.v1.schema.json", spec)

choice_without_options = copy.deepcopy(spec)
choice_without_options["decision_kind"] = "choice"
assert not SCHEMAS["decision-spec.v1.schema.json"].is_valid(choice_without_options)

graph = {
    "format_version": 1,
    "kind": "decision-graph",
    "id": "property.due-diligence",
    "revision": 1,
    "nodes": [
        {"id": "evidence", "spec_id": "property.evidence-completeness", "spec_revision": 1, "depends_on": []},
        {"id": "zoning", "spec_id": "property.zoning-conflict", "spec_revision": 4, "depends_on": ["evidence"]},
        {"id": "summary", "spec_id": "property.development-screening", "spec_revision": 2, "depends_on": ["evidence", "zoning"]},
    ],
    "reducers": [
        {"id": "assessment-summary", "type": "deterministic", "inputs": ["evidence", "zoning", "summary"], "output": "property.assessment-summary"}
    ],
}
validate("decision-graph.v1.schema.json", graph)
assert not graph_semantic_errors(graph)
cyclic = copy.deepcopy(graph)
cyclic["nodes"][0]["depends_on"] = ["summary"]
assert graph_semantic_errors(cyclic)

request = {
    "format_version": 1,
    "kind": "decision-request",
    "request_id": "req_01",
    "state": {
        "schema_id": "lahaku.property-decision-state",
        "schema_version": 3,
        "fingerprint": "sha256:8b930f0c",
        "snapshot_ref": "evidence://property/123/rev/17",
    },
    "questions": [{"spec_id": "property.zoning-conflict", "spec_revision": 4}],
    "mode": "live",
    "provider_hint": "typesafe-jev",
    "budget": {"timeout_ms": 2500, "max_cost": 0.01, "max_parallel": 4},
}
validate("decision-request.v1.schema.json", request)

provider = {
    "format_version": 1,
    "kind": "decision-provider-profile",
    "id": "typesafe-jev",
    "revision": 1,
    "provider_type": "jev",
    "decision_kinds": ["boolean", "choice", "ordinal"],
    "confidence": {"semantics": "provider-defined", "supports_distribution": True},
    "external_network": True,
    "requires_credentials": True,
    "consequence_authority": False,
}
validate("decision-provider-profile.v1.schema.json", provider)
authority = copy.deepcopy(provider)
authority["consequence_authority"] = True
assert not SCHEMAS["decision-provider-profile.v1.schema.json"].is_valid(authority)

policy = {
    "format_version": 1,
    "kind": "decision-policy",
    "id": "property.high-risk-screening",
    "revision": 2,
    "applies_to": ["property.zoning-conflict"],
    "risk": "high",
    "evidence": {"minimum_coverage": 1.0, "on_missing_required": "review"},
    "confidence": {
        "calibration_required": False,
        "minimum_provider_confidence": 0.9,
        "minimum_decision_certainty": None,
    },
    "dispositions": {
        "success": "review",
        "low_confidence": "review",
        "insufficient_evidence": "review",
        "conflict": "review",
        "provider_failure": "review",
    },
    "consequential_action": "separate-authorization-required",
    "invariants": [
        {
            "id": "model-cannot-verify-legal-fact",
            "description": "A semantic result cannot promote an unverified legal claim to verified.",
            "enforcement": "deterministic",
        }
    ],
    "human_review": {"required": True, "role": "authorized-reviewer"},
}
validate("decision-policy.v1.schema.json", policy)

receipt = {
    "format_version": 1,
    "kind": "decision-receipt",
    "id": "dec_01",
    "spec": {"id": "property.zoning-conflict", "revision": 4},
    "state": {
        "schema_id": "lahaku.property-decision-state",
        "schema_version": 3,
        "fingerprint": "sha256:8b930f0c",
    },
    "status": "produced",
    "result": {"value": True, "distribution": {"false": 0.07, "true": 0.93}},
    "provider": {"type": "jev", "id": "typesafe-jev", "model": "system-one", "version": "2026-09"},
    "uncertainty": {
        "provider_confidence": 0.93,
        "calibration": {"status": "uncalibrated"},
        "evidence_coverage": {"value": 1.0, "required_present": 2, "required_total": 2, "missing": []},
        "evidence_reliability": 0.88,
        "decision_certainty": None,
    },
    "evidence": {"used": ["evidence:zoning:1", "evidence:parcel:9"], "missing": []},
    "policy": {
        "id": "property.high-risk-screening",
        "revision": 2,
        "disposition": "review",
        "reasons": ["high-risk decision class requires review"],
    },
    "timing": {"decided_at": "2026-09-18T18:58:00Z", "latency_ms": 87},
}
validate("decision-receipt.v1.schema.json", receipt)
assert not receipt_semantic_errors(receipt)
bad_probability = copy.deepcopy(receipt)
bad_probability["outcome_probability"] = 0.93
assert not SCHEMAS["decision-receipt.v1.schema.json"].is_valid(bad_probability)
bad_coverage = copy.deepcopy(receipt)
bad_coverage["uncertainty"]["evidence_coverage"]["value"] = 0.5
assert receipt_semantic_errors(bad_coverage)

outcome = {
    "format_version": 1,
    "kind": "decision-outcome",
    "id": "out_01",
    "receipt_id": "dec_01",
    "observed_at": "2026-09-19T09:00:00Z",
    "action_ref": "review:property:123:zoning",
    "outcome": {"label": "conflict-confirmed", "value": True, "success": True},
    "verification": {"type": "external-authority", "ref": "evidence:zoning-authority:44"},
    "feedback": {"usable_for_evaluation": True},
}
validate("decision-outcome.v1.schema.json", outcome)

evaluation = {
    "format_version": 1,
    "kind": "decision-evaluation",
    "id": "eval_01",
    "mode": "champion-challenger",
    "dataset": {"id": "zoning-conflict", "revision": 7, "point_in_time": True},
    "champion": {"provider_id": "deterministic-rules", "policy_id": "property.high-risk-screening", "revision": 2},
    "candidate": {"provider_id": "typesafe-jev", "policy_id": "property.high-risk-screening", "revision": 2},
    "side_effects": False,
    "changed_dimensions": ["provider"],
    "source_receipts": ["dec_01"],
    "metrics": [
        {"name": "accuracy", "value": 0.91, "unit": "ratio"},
        {"name": "abstention_rate", "value": 0.08, "unit": "ratio"},
    ],
    "coverage": {"total": 100, "evaluated": 92, "abstained": 8, "failed": 0},
    "generated_at": "2026-09-18T19:00:00Z",
}
validate("decision-evaluation.v1.schema.json", evaluation)
assert not evaluation_semantic_errors(evaluation)
side_effect = copy.deepcopy(evaluation)
side_effect["side_effects"] = True
assert not SCHEMAS["decision-evaluation.v1.schema.json"].is_valid(side_effect)
counterfactual = copy.deepcopy(evaluation)
counterfactual["mode"] = "counterfactual"
counterfactual["changed_dimensions"] = []
assert evaluation_semantic_errors(counterfactual)

print("Decision Kernel v1 schemas, fixtures and semantic invariants passed")
