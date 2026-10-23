"""Live diagnostic reviewer calibration, separate from production acceptance.

Uses synthetic known-good / deliberately corrupted pairs. These labels are
editorial controls, not human adjudication of natural extraction outputs.
"""
import argparse
import concurrent.futures
import hashlib
import fcntl
import json
import os
from pathlib import Path
import stat
import time
import urllib.request


def reserve_evaluation_budget(model, prompt, source, output_limit):
    """Same conservative bound as the Rust evaluator; no refunds for controls."""
    path = os.environ.get("PHASE3_EVALUATION_BUDGET_LEDGER")
    if path is None:
        return True
    if model != "openai/gpt-6-luna-pro":
        return False
    cost = (len(prompt.encode()) + len(json.dumps(source, ensure_ascii=False).encode()) + 4097) // 2 + output_limit
    with open(path, "r+") as ledger:
        fcntl.flock(ledger, fcntl.LOCK_EX)
        state = json.load(ledger)
        accepted = state["reserved_micro_usd"] + cost <= state["limit_micro_usd"]
        if accepted:
            state["reserved_micro_usd"] += cost
        else:
            state["rejected_requests"] = state.get("rejected_requests", 0) + 1
        ledger.seek(0)
        ledger.truncate()
        json.dump(state, ledger)
        ledger.flush()
        os.fsync(ledger.fileno())
    return accepted

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
    credential = Path(os.environ["OPENROUTER_API_KEY_FILE"])
    info = credential.lstat()
    if credential != credential.resolve() or not stat.S_ISREG(info.st_mode) or stat.S_IMODE(info.st_mode) != 0o600 or info.st_uid != os.getuid() or info.st_nlink != 1:
        raise SystemExit("A canonical private 0600 credential file is required")
    key = credential.read_text().strip()
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
            if version in {"extraction-v10", "extraction-v11", "extraction-v12", "extraction-v13", "extraction-v14", "extraction-v15", "extraction-v16", "extraction-v17", "extraction-v18", "extraction-v19", "extraction-v20"}:
                source["candidate_count"] = len(source["candidate_batch"]["claims"])
                source["required_checked_indices"] = list(range(source["candidate_count"]))
        payload = dict(model=args.model, temperature=0, max_tokens=args.max_output_tokens, response_format=dict(type="json_object"),
                       messages=[dict(role="system", content=prompt), dict(role="user", content=json.dumps(source, ensure_ascii=False))])
        result = dict(control=control, version=version, model=args.model, stage=stage, run=run,
                      prompt_hash=hashlib.sha256(prompt.encode()).hexdigest(), max_output_tokens=args.max_output_tokens)
        if "deepseek" in args.model.lower() and not (prompt + json.dumps(source, ensure_ascii=False)).isascii():
            result["error"] = "LanguageRestriction"
            return result
        try:
            allowed = reserve_evaluation_budget(args.model, prompt, source, args.max_output_tokens)
        except (OSError, ValueError, KeyError, TypeError):
            allowed = False
        if not allowed:
            result["error"] = "LocalBudget"
            return result
        request = urllib.request.Request("https://openrouter.ai/api/v1/chat/completions",
                                         data=json.dumps(payload).encode(),
                                         headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"})
        start = time.monotonic()
        try:
            with urllib.request.urlopen(request, timeout=180) as response:
                data = response.read(8 * 1024 * 1024 + 1)
                if len(data) > 8 * 1024 * 1024:
                    raise ValueError()
                response_data = json.loads(data)
                choice = response_data["choices"][0]
                reason = choice.get("finish_reason")
                result["finish_reason"] = reason if reason in {"stop", "length", "tool_calls", "content_filter"} else "other"
                usage = response_data.get("usage", {})
                result["usage"] = {k: usage[k] for k in ["prompt_tokens", "completion_tokens", "total_tokens"] if type(usage.get(k)) is int}
                if choice.get("finish_reason") != "stop":
                    raise ValueError()
                result["raw"] = choice["message"]["content"]
        except Exception as error:
            result["error"] = type(error).__name__  # Never expose HTTP bodies or credentials.
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
