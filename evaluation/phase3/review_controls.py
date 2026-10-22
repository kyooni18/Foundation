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
import stat
import time
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--runs", type=int, default=2)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    credential = Path(os.environ["OPENROUTER_API_KEY_FILE"])
    info = credential.lstat()
    if credential != credential.resolve() or not stat.S_ISREG(info.st_mode) or stat.S_IMODE(info.st_mode) != 0o600 or info.st_uid != os.getuid() or info.st_nlink != 1:
        raise SystemExit("A canonical private 0600 credential file is required")
    key = credential.read_text().strip()
    controls = json.loads(Path(__file__).with_name("reviewer-controls.json").read_text())
    stages = ["atomicity and independence", "semantic fidelity and epistemic framing",
              "whole-batch coverage, duplication and conflicting interpretation"]
    tasks = [(version, control, stage, run) for version in ["extraction-v1", "extraction-v2"]
             for control in controls for stage in stages for run in range(args.runs)]

    def evaluate(task):
        version, control, stage, run = task
        folder = root / "prompts" / version
        prompt = (folder / "review.txt").read_text() + "\n" + (folder / "frames.txt").read_text() + f"\nIndependent review responsibility: {stage}. Compare against original, never a translation."
        source = {k: control[k] for k in ["original_source", "candidate_batch"]}
        source["authored_at"] = None
        payload = dict(model=args.model, temperature=0, response_format=dict(type="json_object"),
                       messages=[dict(role="system", content=prompt), dict(role="user", content=json.dumps(source, ensure_ascii=False))])
        result = dict(control=control, version=version, model=args.model, stage=stage, run=run,
                      prompt_hash=hashlib.sha256(prompt.encode()).hexdigest())
        request = urllib.request.Request("https://openrouter.ai/api/v1/chat/completions",
                                         data=json.dumps(payload).encode(),
                                         headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"})
        start = time.monotonic()
        try:
            with urllib.request.urlopen(request, timeout=180) as response:
                data = response.read(8 * 1024 * 1024 + 1)
                if len(data) > 8 * 1024 * 1024:
                    raise ValueError()
                result["raw"] = json.loads(data)["choices"][0]["message"]["content"]
        except Exception as error:
            result["error"] = type(error).__name__  # Never expose HTTP bodies or credentials.
        result["elapsed_ms"] = round((time.monotonic() - start) * 1000)
        return result

    descriptor = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w") as output, concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        futures = [pool.submit(evaluate, task) for task in tasks]
        for future in concurrent.futures.as_completed(futures):
            result = future.result()
            output.write(json.dumps(result, ensure_ascii=False) + "\n")
            output.flush()
            os.fsync(output.fileno())
            print(result["version"], result["control"]["id"], result["stage"], result["run"], flush=True)


if __name__ == "__main__":
    main()
