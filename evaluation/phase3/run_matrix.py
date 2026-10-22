#!/usr/bin/env python3
"""Bounded live evaluation; credentials are inherited, never serialized.

Each shard is an independently durable JSONL artifact. A failed/interrupted shard
is retained for diagnosis; use a new output directory for another experiment.
"""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--reviewer", required=True)
    parser.add_argument("--prompt-version")
    parser.add_argument("--split", default="dev")
    parser.add_argument("--runs", type=int, default=2)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--variants", default="single,r2-frame,decompose,english-intermediate,atomicity-review,fidelity-review,audited")
    args = parser.parse_args()
    if not 1 <= args.workers <= 8 or not 1 <= args.runs <= 10:
        parser.error("workers must be 1..8; runs must be 1..10")
    root = Path(__file__).resolve().parents[2]
    executable = root / "target/debug/examples/evaluate_extraction"
    args.output.mkdir(mode=0o700)
    cases = [c for c in json.loads(args.corpus.read_text()) if c["split"] == args.split]
    if not cases or len({c["id"] for c in cases}) != len(cases):
        parser.error("corpus must have unique IDs and a nonempty selected split")
    variants = args.variants.split(",")
    tasks = []
    for part, start in enumerate(range(0, len(cases), 8)):
        corpus = args.output / f"corpus-{part}.json"
        corpus.write_text(json.dumps(cases[start:start + 8], ensure_ascii=False, indent=2))
        for variant in variants:
            tasks.append((part, variant, corpus))
    manifest = vars(args).copy()
    manifest.update(expected_rows=len(cases) * len(variants) * args.runs,
                    git_commit=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip())
    (args.output / "manifest.json").write_text(json.dumps(manifest, default=str, indent=2))

    def run(task):
        part, variant, corpus = task
        stem = args.output / f"{part}-{variant}"
        cmd = [str(executable), "--corpus", str(corpus), "--output", str(stem) + ".jsonl",
               "--model", args.model, "--reviewer", args.reviewer,
               "--split", args.split, "--variants", variant, "--runs", str(args.runs)]
        if args.prompt_version:
            cmd += ["--prompt-version", args.prompt_version]
        with open(str(stem) + ".log", "x") as log:
            status = subprocess.run(cmd, stdout=log, stderr=log, env=os.environ).returncode
        print(f"completed shard={part} variant={variant} exit={status}", flush=True)
        return status

    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        statuses = list(pool.map(run, tasks))
    if any(statuses):
        raise SystemExit("Incomplete experiment; inspect shard logs")
    rows = [json.loads(line) for path in sorted(args.output.glob("*.jsonl"))
            for line in path.read_text().splitlines()]
    if len(rows) != manifest["expected_rows"]:
        raise SystemExit("Incomplete row count")
    with (args.output / "results.jsonl").open("x") as output:
        for row in rows:
            output.write(json.dumps(row, ensure_ascii=False) + "\n")
    print(f"Saved {len(rows)} results; semantic adjudication still required", flush=True)


if __name__ == "__main__":
    main()
