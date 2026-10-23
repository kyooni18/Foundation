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
    def test_concurrent_control_requests_share_one_ceiling(self):
        with tempfile.TemporaryDirectory() as folder:
            ledger = Path(folder) / 'budget.json'
            ledger.write_text(json.dumps({'limit_micro_usd': 3000, 'reserved_micro_usd': 0}))
            with patch.dict(os.environ, {'PHASE3_EVALUATION_BUDGET_LEDGER': str(ledger)}):
                with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
                    outcomes = list(pool.map(lambda _: review_controls.reserve_evaluation_budget(
                        'openai/gpt-6-luna-pro', 'JSON', {'claim_text': '원문'}, 1), range(8)))
            state = json.loads(ledger.read_text())
            self.assertEqual(outcomes.count(True), 1)
            self.assertEqual(state['rejected_requests'], 7)
            self.assertLessEqual(state['reserved_micro_usd'], 3000)
