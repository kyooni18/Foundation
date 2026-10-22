"""Focused measurement regression; synthetic labels are not model-quality evidence."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("score.py")
FLAGS = ["atomicity_violation", "unsupported", "fidelity_error", "epistemic_error",
         "language_error", "anchor_error", "duplicate"]
GROUP = "extraction-v1|unspecified|unspecified|audited|ko|all"

class ScoringTests(unittest.TestCase):
    def run_score(self, rows):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "rows.jsonl"
            path.write_text("".join(json.dumps(row) + "\n" for row in rows))
            return subprocess.run([sys.executable, "-O", str(SCRIPT), str(path)],
                                  capture_output=True, text=True)

    def test_denominators_and_incomplete_annotations(self):
        row = dict(case_id="synthetic", variant="audited", run=0, prompt_hash="p",
                   source_hash="s", language="ko", category="scope", structural_valid=True,
                   raw=json.dumps({"claims": [{"text": "synthetic"}]}), reviews=[],
                   human_review=dict(reviewer="test fixture", accepted=False,
                                     supported_propositions=2, covered_propositions=1,
                                     claims=[{flag: False for flag in FLAGS}],
                                     oversplits=0, decompositions=1,
                                     translation_errors=0, translation_candidates=0))
        result = self.run_score([row])
        self.assertEqual(result.returncode, 0, result.stderr)
        group = json.loads(result.stdout)[GROUP]
        self.assertEqual(group["coverage"], dict(numerator=0, denominator=2, rate=0.0))
        self.assertIsNone(group["atomicity_violation"]["rate"])
        missing = copy.deepcopy(row)
        missing["human_review"] = None
        group = json.loads(self.run_score([missing]).stdout)[GROUP]
        self.assertEqual(group["unreviewed"], 1)
        self.assertIsNone(group["coverage"]["rate"])
        mutations = [
            lambda r: r["human_review"].update(claims=[]),
            lambda r: r["human_review"].update(covered_propositions=3),
            lambda r: r["human_review"].update(oversplits=-1),
            lambda r: r["human_review"]["claims"][0].update(unsupported="false"),
            lambda r: (r.update(structural_valid=False), r["human_review"].update(accepted=True)),
        ]
        for mutate in mutations:
            bad = copy.deepcopy(row)
            mutate(bad)
            self.assertNotEqual(self.run_score([bad]).returncode, 0)
        self.assertNotEqual(self.run_score([row, row]).returncode, 0)

if __name__ == "__main__":
    unittest.main()
