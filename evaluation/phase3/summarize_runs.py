"""Summarize observable live-run diagnostics, NEVER semantic ground truth."""
import argparse
import collections
import gzip
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("inputs", nargs="+", type=Path)
parser.add_argument("--current-validation", action="store_true", help="Use explicitly saved retrospective validation; preserve raw run results")
args = parser.parse_args()
rows = []
for path in args.inputs:
    reader = gzip.open if path.suffix == ".gz" else open
    with reader(path, "rt") as source:
        rows.extend(json.loads(line) for line in source)


def review_pass(review, count):
    try:
        value = json.loads(review["raw"])
        return (set(value) == {"verdict", "issues", "checked", "coverage_complete"}
                and value["verdict"] == "pass" and value["issues"] == []
                and value["coverage_complete"] is True
                and isinstance(value["checked"], list)
                and all(type(index) is int for index in value["checked"])
                and sorted(value["checked"]) == list(range(count)))
    except (KeyError, TypeError, ValueError):
        return False


groups = collections.defaultdict(list)
for row in rows:
    version = row["plan"].get("prompt_version") or "extraction-v1"
    for language in [row["language"], "all"]:
        groups[(version, row["plan"]["extractor"]["model"], row["plan"]["reviewer"]["model"], row["variant"], language)].append(row)
output = {"warning": "Model gate results and lexical variation are diagnostics, not semantic quality acceptance.", "groups": {}}
for key, values in groups.items():
    stages = {"audited": 3, "english-staged": 3, "atomicity-review": 1, "fidelity-review": 1}.get(key[-2], 0)
    per_case = collections.defaultdict(list)
    issues = collections.Counter()
    gate_passes = 0
    recovered = 0
    terminal_failures = 0
    generated = 0
    for row in values:
        reviews = row.get("reviews", [])
        reader_count = row.get("claim_count", 0) if key[0] in {"extraction-v11", "extraction-v12", "extraction-v13", "extraction-v14", "extraction-v15", "extraction-v16", "extraction-v17", "extraction-v18", "extraction-v19", "extraction-v20", "extraction-v21"} and key[-2] in {"audited", "english-staged"} else 0
        gate = bool(row.get("structural_valid") and len(reviews) == stages + reader_count
                    and all(review_pass(review, 1 if review.get("stage", "").startswith("standalone") else row["claim_count"]) for review in reviews))
        if args.current_validation:
            if "revalidation" not in row:
                raise ValueError("Explicit retrospective validation required for every row")
            gate = row["revalidation"]["experimental_gate_eligible"]
        gate_passes += gate
        recovered += bool(gate and row.get("attempt", 1) > 1)
        terminal_failures += bool(row.get("terminal", True) and not gate)
        try:
            texts = [claim["text"] for claim in json.loads(row["raw"])["claims"]]
            generated += len(texts)
            if row.get("terminal", True):
                per_case[row["case_id"]].append(texts)
        except (KeyError, TypeError, ValueError):
            pass
        for review in reviews:
            try:
                verdict = json.loads(review["raw"])
                for issue in verdict.get("issues", []):
                    issues[issue] += 1
            except (KeyError, TypeError, ValueError):
                issues["provider_or_schema_error"] += 1
    output["groups"]["|".join(key)] = dict(
        runs=len({(row['case_id'],row['run'],row['source_hash'],row['prompt_hash']) for row in values}),
        attempt_rows=len(values), terminal_runs=sum(row.get('terminal',True) for row in values),
        structural_valid=sum(row.get("structural_valid") is True for row in values),
        provider_failures=sum("provider_error" in row or any("error" in review for review in row.get("reviews", [])) for row in values),
        extractor_provider_failures=sum("provider_error" in row for row in values),
        reviewer_provider_error_calls=dict(collections.Counter(review["error"] for row in values for review in row.get("reviews", []) if "error" in review)),
        experimental_gate_passes=gate_passes, reviewer_reported_issues=dict(issues),
        generated_claims=generated, recovered_after_retry=recovered, terminal_gate_failures=terminal_failures,
        repeatability={case: dict(runs=len(batches),
                                 distinct_text_sets=len({json.dumps(sorted(texts), ensure_ascii=False) for texts in batches}),
                                 min_count=min(map(len, batches)), max_count=max(map(len, batches)))
                       for case, batches in per_case.items()})
print(json.dumps(output, ensure_ascii=False, indent=2))
