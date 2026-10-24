"""Verify an inert policy proposal on temporary copies; never open the real ledger."""
import importlib.util
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest

import guarded_haiku as current


class PolicyProposalTest(unittest.TestCase):
    def test_exact_proposal_preserves_history_and_bounds_aggregate_spending(self):
        proposal = Path(__file__).resolve().parent / 'proposed-policy'
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ('src/budget.rs', 'evaluation/phase3/guarded_haiku.py'):
                dest = root / name
                dest.parent.mkdir(parents=True, exist_ok=True)
                # Preserve the reviewed pre-amendment input, even after its patch is applied.
                dest.write_bytes(subprocess.check_output(
                    ['git', 'show', '1e2a74b:' + name], cwd=current.focused.ROOT))
            subprocess.run(['git', 'apply', str(proposal / 'ceiling.patch')], cwd=root, check=True)
            spec = importlib.util.spec_from_file_location('proposed_guard', root / 'evaluation/phase3/guarded_haiku.py')
            proposed = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(proposed)
            self.assertEqual(current.LIMIT, 4_982_653_350)
            self.assertEqual(proposed.LIMIT, 4_882_653_350)
            self.assertIn('const MAX_LIMIT: i64 = 4_882_653_350;', (root / 'src/budget.rs').read_text())
            conn = sqlite3.connect(root / 'fixture.sqlite')
            self.addCleanup(conn.close)
            conn.executescript('CREATE TABLE budget(id INTEGER PRIMARY KEY, ceiling INTEGER, prior INTEGER);'
                'INSERT INTO budget VALUES(1,4500000000,4782653350);'
                'CREATE TABLE request(id TEXT, estimate INTEGER, actual INTEGER);')
            sql = (proposal / 'ceiling.sql').read_text()
            conn.executescript(sql)
            self.assertEqual(conn.execute('SELECT * FROM budget').fetchone(), (1,4882653350,4782653350))
            # check() has no transport or mutation; exercise exact proposed arithmetic.
            guard = object.__new__(proposed.Dispatcher)
            self.assertEqual(guard.check(conn, 100_000_000)[1], 4_882_653_350)
            with self.assertRaisesRegex(ValueError, 'budget blocked'):
                guard.check(conn, 100_000_001)
            self.assertEqual(object.__new__(current.Dispatcher).check(conn, 1)[0], proposed.LIMIT)
            conn.execute("INSERT INTO request VALUES('other-worker',99999999,NULL)")
            guard.check(conn, 1)
            with self.assertRaisesRegex(ValueError, 'budget blocked'):
                guard.check(conn, 2)
            conn.commit()
            before = conn.execute('SELECT * FROM request').fetchall()
            with self.assertRaises(sqlite3.IntegrityError):
                conn.executescript(sql)
            conn.rollback()
            self.assertEqual(conn.execute('SELECT * FROM request').fetchall(), before)
            self.assertEqual(conn.execute('SELECT prior FROM budget').fetchone()[0], 4782653350)
