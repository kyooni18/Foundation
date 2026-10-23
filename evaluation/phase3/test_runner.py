"""Operational regression: exhausted shards must not dispatch the remaining matrix."""
import concurrent.futures
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import run_matrix
import review_controls


class RunnerTest(unittest.TestCase):
    def test_failed_shard_stops_pending_dispatch_and_preserves_incomplete_manifest(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            corpus = root / 'corpus.json'
            corpus.write_text(json.dumps([{'id': str(i), 'split': 'dev'} for i in range(4)]))
            output = root / 'run'
            argv = ['run_matrix', '--corpus', str(corpus), '--output', str(output),
                    '--model', 'fake', '--reviewer', 'fake', '--workers', '1',
                    '--shard-size', '1', '--variants', 'audited']
            with patch.object(sys, 'argv', argv), \
                    patch.object(run_matrix.subprocess, 'check_output', return_value='checkpoint'), \
                    patch.object(run_matrix.subprocess, 'run') as execute:
                execute.return_value.returncode = 1
                with self.assertRaises(SystemExit):
                    run_matrix.main()
            self.assertEqual(execute.call_count, 1)
            manifest = json.loads((output / 'manifest.json').read_text())
            self.assertEqual(manifest['status'], 'incomplete')
            self.assertEqual(manifest['failed_shards'], 1)
            self.assertEqual(manifest['skipped_shards'], 3)
            self.assertFalse((output / 'results.jsonl').exists())


class BudgetTest(unittest.TestCase):
    def test_controls_use_guarded_adapter_without_shell_or_secret_output(self):
        with patch.object(review_controls.subprocess, "run") as execute:
            execute.return_value.returncode = 1
            execute.return_value.stderr = "sensitive provider body"
            result = review_controls.guarded_completion(Path("/tmp"), "model", "instructions", {"source": "$(do-not-execute) 한국어"}, 100)
            self.assertEqual(result, {"error": "ProviderOrBudget"})
            args, kwargs = execute.call_args
            self.assertEqual(args[0], ["/tmp/target/debug/examples/guarded_completion"])
            self.assertNotIn("shell", kwargs)
            self.assertEqual(json.loads(kwargs["input"])["config"]["max_output_tokens"], 100)
            execute.return_value.returncode = 0
            execute.return_value.stdout = '{"raw":"{}","usage":{"prompt_tokens":2}}'
            self.assertEqual(review_controls.guarded_completion(Path("/tmp"), "model", "p", {}, 1)["raw"], "{}")
