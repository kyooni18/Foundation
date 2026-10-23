"""Live diagnostic reviewer calibration, separate from production acceptance.

Uses synthetic known-good / deliberately corrupted pairs. These labels are
editorial controls, not human adjudication of natural extraction outputs.
"""
import argparse
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import time
import subprocess


def guarded_completion(root, model, prompt, source, output_limit):
    """No independent HTTP or accounting path: use the production Rust adapter."""
    request = dict(system=prompt, input=json.dumps(source, ensure_ascii=False),
                   config=dict(model=model, timeout_seconds=180, max_output_tokens=output_limit))
    process = subprocess.run([str(root / "target/debug/examples/guarded_completion")],
                             input=json.dumps(request, ensure_ascii=False), text=True,
                             capture_output=True, timeout=225, check=False)
    if process.returncode:
        return {"error": "ProviderOrBudget"}
    return json.loads(process.stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--runs", type=int, default=2)
    parser.add_argument("--versions", default="extraction-v1,extraction-v2")
    parser.add_argument("--standalone-prompt", type=Path)
    parser.add_argument("--controls", type=Path, default=Path(__file__).with_name("reviewer-controls.json"))
    parser.add_argument("--max-output-tokens", type=int, default=4096)
    parser.add_argument("--workers", type=int, default=6)
    args = parser.parse_args()
    if not 1 <= args.workers <= 8 or not 1 <= args.runs <= 10 or not 1 <= args.max_output_tokens <= 262144:
        parser.error("invalid bounded evaluation configuration")
    root = Path(__file__).resolve().parents[2]
    controls = json.loads(args.controls.read_text())
    stages = (["standalone atomicity and isolation"] if args.standalone_prompt else
              ["atomicity and independence", "semantic fidelity and epistemic framing",
               "whole-batch coverage, duplication and conflicting interpretation"])
    tasks = [(version, control, stage, run) for version in args.versions.split(",")
             for control in controls for stage in stages for run in range(args.runs)]

    def evaluate(task):
        version, control, stage, run = task
        if args.standalone_prompt:
            prompt = args.standalone_prompt.read_text()
            claims = control["candidate_batch"]["claims"]
            if len(claims) != 1:
                raise ValueError("Standalone controls must contain exactly one Claim")
            source = {"claim_text": claims[0]["text"], "candidate_count": 1, "required_checked_indices": [0]}
        else:
            folder = root / "prompts" / ("extraction-v12" if version == "extraction-v13" else version)
            review_folder = root / "prompts" / ("extraction-v12" if version in {"extraction-v13", "extraction-v14"} else version)
            prompt = (review_folder / "review.txt").read_text() + "\n" + (folder / "frames.txt").read_text() + f"\nIndependent review responsibility: {stage}. Compare against original, never a translation."
            source = {k: control[k] for k in ["original_source", "candidate_batch"]}
            source["authored_at"] = control.get("authored_at")
            if version in {"extraction-v10", "extraction-v11", "extraction-v12", "extraction-v13", "extraction-v14", "extraction-v15", "extraction-v16", "extraction-v17", "extraction-v18", "extraction-v19", "extraction-v20", "extraction-v21"}:
                source["candidate_count"] = len(source["candidate_batch"]["claims"])
                source["required_checked_indices"] = list(range(source["candidate_count"]))
        result = dict(control=control, version=version, model=args.model, stage=stage, run=run,
                      prompt_hash=hashlib.sha256(prompt.encode()).hexdigest(), max_output_tokens=args.max_output_tokens)
        if "deepseek" in args.model.lower() and not (prompt + json.dumps(source, ensure_ascii=False)).isascii():
            result["error"] = "LanguageRestriction"
            return result
        start = time.monotonic()
        try:
            result.update(guarded_completion(root, args.model, prompt, source, args.max_output_tokens))
        except Exception as error:
            result["error"] = type(error).__name__  # Never expose bodies or credentials.
        result["elapsed_ms"] = round((time.monotonic() - start) * 1000)
        return result

    descriptor = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w") as output, concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = [pool.submit(evaluate, task) for task in tasks]
        for future in concurrent.futures.as_completed(futures):
            result = future.result()
            output.write(json.dumps(result, ensure_ascii=False) + "\n")
            output.flush()
            os.fsync(output.fileno())
            print(result["version"], result["control"]["id"], result["stage"], result["run"], flush=True)


if __name__ == "__main__":
    main()
