#!/usr/bin/env python3
"""Measure synthetic source selection, not model quality or provider billing."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

from jsonschema import Draft202012Validator


def estimate(text: str) -> int:
    return max(1, (len(text) + 3) // 4)


def run(binary: Path, schema_path: Path) -> dict:
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)
    manifest = {
        "format_version": 1,
        "project": {"name": "fixture", "type": "base", "maturity": "prototype"},
        "context": {"product": "PRODUCT.md", "architecture": "ARCHITECTURE.md",
                    "security": "SECURITY.md", "decisions": "decisions/"},
        "modules": {"packs": [], "policies": []}, "skills": [], "permissions": {},
        "adapters": {"canonical_router": "../AGENTS.md", "vendor_files_must_be_thin": True},
    }
    files = {
        "AGENTS.md": "Preserve required rules. Read task-relevant sources.",
        ".agentic/manifest.yaml": json.dumps(manifest, sort_keys=True),
        ".agentic/PRODUCT.md": "A synthetic project administration tool.",
        ".agentic/ARCHITECTURE.md": "Keep validation separate from transport.",
        ".agentic/SECURITY.md": "Never expose credentials or bypass permissions.",
        "src/githubProject.ts": "export function validateProjectName(name: string) { return name.trim().length > 0; }",
    }
    for index in range(10):
        files[f"src/palette{index}.ts"] = "// unrelated palette rendering observations\n" * 500
    required = set(files) - {p for p in files if p.startswith("src/palette")}
    with tempfile.TemporaryDirectory(prefix="ah-context-efficiency-") as directory:
        root = Path(directory)
        for relative, content in files.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content.encode("utf-8"))
        command = [str(binary), "agentic", "context", ".", "--task",
                   "validate GitHub project name", "--max-tokens", "4000"]
        def invoke():
            result = subprocess.run(command, cwd=root, capture_output=True, timeout=60, check=True)
            value = json.loads(result.stdout)
            validator.validate(value)
            return value, result.stdout.decode("utf-8")
        plan, raw = invoke()
        repeated, _ = invoke()
        assert plan == repeated, "unchanged inputs must be deterministic"
        assert plan["coverage"]["complete"] is True
        assert plan["coverage"]["required_unavailable"] == 0
        items = plan["items"]
        assert len({item["id"] for item in items}) == len(items)
        assert {item["source"]["path"] for item in items} == set(files)
        selected = {item["source"]["path"] for item in items if item["disposition"] == "included"}
        assert required <= selected, "optimisation dropped required fixture evidence"
        for item in items:
            content = files[item["source"]["path"]]
            assert item["source"]["digest"] == "sha256:" + hashlib.sha256(content.encode()).hexdigest()
            assert item["estimated_tokens"] == estimate(content)
            assert not item["mandatory"] or item["disposition"] == "included"
        total = sum(item["estimated_tokens"] for item in items if item["disposition"] == "included")
        assert plan["budget"]["input"]["estimated"] == total
        assert plan["budget"]["over_budget"] is (total > 4000)
        baseline = sum(estimate(content) for content in files.values())
        assert total < baseline, "synthetic irrelevant source was not excluded"
    version = json.loads(subprocess.run([str(binary), "--version"], capture_output=True, timeout=60, check=True).stdout)
    fixture = json.dumps(files, sort_keys=True, separators=(",", ":")).encode()
    return {
        "format_version": 1,
        "kind": "context-efficiency-probe",
        "scope": "synthetic source-selection only; not end-to-end inference",
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "sources": version["sources"],
        "fixture_sha256": hashlib.sha256(fixture).hexdigest(),
        "compiler": plan["compiler"],
        "files": len(files), "selected_files": len(selected),
        "required_fixture_files_retained": len(required),
        "baseline_source_tokens_estimated": baseline,
        "selected_source_tokens_estimated": total,
        "plan_report_tokens_estimated": estimate(raw),
        "source_reduction_fraction": 1 - total / baseline,
        "observed_provider_tokens": None,
        "observed_provider_cost_usd": None,
        "model_quality_change": None,
        "checks": ["canonical schema", "exact source digests", "budget arithmetic",
                   "deterministic rerun", "required fixture retention"],
        "limitations": ["characters/4 is not a provider tokenizer",
                        "report overhead is measured separately and is not free",
                        "no host context injection, Jev inference or task-outcome evaluation"],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    schema_path = Path(__file__).resolve().parents[1] / "upstream/agentic-harness/catalog/schema/compiled-context-plan.v1.schema.json"
    print(json.dumps(run(args.binary.resolve(strict=True), schema_path), sort_keys=True))


if __name__ == "__main__":
    main()
