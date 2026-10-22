"""Summarize observable live-run diagnostics, NEVER semantic ground truth."""
import argparse
import collections
import gzip
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("inputs", nargs="+", type=Path)
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
    for row in values:
        reviews = row.get("reviews", [])
        gate_passes += bool(row.get("structural_valid") and len(reviews) == stages
                            and all(review_pass(review, row["claim_count"]) for review in reviews))
        try:
            texts = [claim["text"] for claim in json.loads(row["raw"])["claims"]]
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
        runs=len(values), structural_valid=sum(row.get("structural_valid") is True for row in values),
        provider_failures=sum("provider_error" in row for row in values),
        experimental_gate_passes=gate_passes, reviewer_reported_issues=dict(issues),
        generated_claims=sum(row.get("claim_count", 0) for row in values),
        repeatability={case: dict(runs=len(batches),
                                 distinct_text_sets=len({json.dumps(sorted(texts), ensure_ascii=False) for texts in batches}),
                                 min_count=min(map(len, batches)), max_count=max(map(len, batches)))
                       for case, batches in per_case.items()})
print(json.dumps(output, ensure_ascii=False, indent=2))
