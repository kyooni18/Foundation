import copy
from contextlib import contextmanager
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

import guarded_haiku as g


class GuardedHaikuTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.ledger = self.root / 'fixture.sqlite'
        with self.connection() as conn:
            conn.executescript('CREATE TABLE budget(id INTEGER PRIMARY KEY, ceiling INTEGER, prior INTEGER);'
                'INSERT INTO budget VALUES(1,4500000000,4000000000);'
                'CREATE TABLE request(id TEXT PRIMARY KEY,model TEXT,created_at TEXT,estimate INTEGER,'
                'actual INTEGER,provider_id TEXT,settled_at TEXT,cumulative INTEGER,prompt_tokens INTEGER,completion_tokens INTEGER);')
        snapshot = json.loads((g.focused.ROOT / 'reports/evidence/phase-3/focused-continuation/model-availability.json').read_text())
        self.catalog = {'data': snapshot['catalog']['data']}
        self.endpoints = snapshot['endpoints']['data']
        self.packet = g.packets()[1][0]
        self.calls = []
        source = self.packet['case']['source']
        evidence = [{'start': 0, 'end': len(source), 'quote': source}]
        # Synthetic unresolved fixture, NOT a semantic result.
        self.judgment = dict(verdict='unresolved', uncertainty_flags=['synthetic fixture'],
            claims=[dict(index=i, text=c['text'], evidence=evidence, uncertainty_flags=[],
                checks={k: dict(status='unresolved', reason='synthetic fixture') for k in g.focused.CHECKS})
                for i, c in enumerate(self.packet['candidate']['claims'])],
            inventory=[dict(reference_index=i, status='unresolved', claim_indices=[], evidence=evidence,
                            reason='synthetic fixture') for i, _ in enumerate(self.packet['case']['reference']['propositions'])])
        self.response = dict(id='fixture-id', usage=dict(cost='0.001', prompt_tokens=100, completion_tokens=100),
            choices=[dict(finish_reason='stop', message=dict(content=json.dumps(self.judgment)))])
        self.auth = dict(explicit_user_authorization=True, additional_limit_nanos=g.CONTINUATION,
                         strict_total_below_nanos=5_000_000_000, authorization_reference='TEST ONLY')

    @contextmanager
    def connection(self):
        conn = sqlite3.connect(self.ledger)
        try:
            with conn:
                yield conn
        finally:
            conn.close()

    def transport(self, method, path, body):
        self.calls.append((method, path, body))
        if path == '/models':
            return self.catalog
        if path.endswith('/endpoints'):
            return self.endpoints
        if path == '/key':
            return {'data': dict(limit=4.5, limit_reset=None, limit_remaining=0.5)}
        with self.connection() as conn:
            self.assertEqual(conn.execute('SELECT COUNT(*) FROM request WHERE actual IS NULL').fetchone()[0], 1)
        self.assertEqual(len(list((self.root / 'evidence').glob('*.request.json'))),
                         sum(c[0] == 'POST' for c in self.calls))
        return self.response

    def dispatcher(self):
        return g.Dispatcher(self.ledger, self.root / 'evidence', self.transport, self.auth)

    def test_snapshot_pricing_parameters_and_immutable_success(self):
        q = g.quote(self.catalog, self.endpoints, 1000)
        self.assertEqual(q['input'], '0.000001')  # highest long-context cache tier
        self.assertEqual(q['output'], '0.0000025')
        self.assertEqual(q['estimate'], 22192000)  # doubled conservative reservation
        result = self.dispatcher().dispatch(self.packet)
        self.assertEqual(result, self.judgment)
        body = self.calls[-1][2]
        self.assertNotIn('temperature', body)
        self.assertEqual(body['reasoning'], {'effort': 'low'})
        self.assertEqual(body['max_tokens'], 2400)
        self.assertFalse(body['provider']['allow_fallbacks'])
        with self.assertRaisesRegex(ValueError, 'already attempted'):
            self.dispatcher().dispatch(self.packet)
        self.assertEqual(sum(c[0] == 'POST' for c in self.calls), 1)
        with self.assertRaises(FileExistsError):
            g.immutable(next((self.root / 'evidence').glob('*.response.json')), {})

    def test_exhausted_existing_budget_blocks_all_transport(self):
        with self.connection() as conn:
            conn.execute('UPDATE budget SET prior=4782653350')
        before = self.ledger.read_bytes()
        with self.assertRaisesRegex(ValueError, 'budget blocked'):
            self.dispatcher().dispatch(self.packet)
        self.assertEqual(self.calls, [])
        self.assertEqual(before, self.ledger.read_bytes())

    def test_timeout_retains_hold_and_no_retry(self):
        original = self.transport
        def timeout(method, path, body):
            if method == 'POST':
                raise TimeoutError('fixture')
            return original(method, path, body)
        dispatcher = g.Dispatcher(self.ledger, self.root / 'evidence', timeout, self.auth)
        with self.assertRaises(TimeoutError):
            dispatcher.dispatch(self.packet)
        with self.connection() as conn:
            self.assertIsNone(conn.execute('SELECT actual FROM request').fetchone()[0])
        with self.assertRaisesRegex(ValueError, 'already attempted'):
            dispatcher.dispatch(self.packet)

    def test_invalid_judgment_is_accounted_but_never_accepted(self):
        self.response['choices'][0]['finish_reason'] = 'length'
        with self.assertRaisesRegex(ValueError, 'incomplete judgment'):
            self.dispatcher().dispatch(self.packet)
        self.assertFalse(list((self.root / 'evidence').glob('*.judgment.json')))
        with self.connection() as conn:
            self.assertEqual(conn.execute('SELECT actual FROM request').fetchone()[0], 1000000)

    def test_unknown_prices_parameters_and_unauthorized_calls_fail_closed(self):
        for mutation in ('unknown_fee', 'reasoning', 'tier'):
            endpoints = copy.deepcopy(self.endpoints)
            for endpoint in endpoints['data']['endpoints']:
                if mutation == 'unknown_fee':
                    endpoint['pricing']['unrecognized'] = '0.01'
                elif mutation == 'reasoning':
                    endpoint['supported_parameters'] = []
                else:
                    endpoint['pricing']['overrides'] = [{'min_context': 12, 'prompt': '1', 'completion': '1'}]
            with self.assertRaises(ValueError):
                g.quote(self.catalog, endpoints, 1000)
        with self.assertRaisesRegex(ValueError, 'authorization missing'):
            g.Dispatcher(self.ledger, self.root / 'evidence', self.transport, {})
        altered = copy.deepcopy(self.packet)
        altered['candidate']['claims'][0]['text'] = 'tampered'
        with self.assertRaisesRegex(ValueError, 'frozen'):
            self.dispatcher().dispatch(altered)
        self.assertFalse(self.calls)

    def test_continuation_limit_and_overrun_block(self):
        with self.connection() as conn:
            conn.execute("INSERT INTO request(id,estimate,actual) VALUES('focused-haiku-other',100000000,100000000)")
        with self.assertRaisesRegex(ValueError, 'budget blocked'):
            self.dispatcher().dispatch(self.packet)
        self.assertFalse(self.calls)

    def test_evidence_protocol_contains_historical_verdict_flip(self):
        record = json.loads((g.focused.ROOT / 'reports/evidence/phase-3/focused-continuation/reviewer-disagreement.json').read_text())
        # Actual legacy responses lack exact Claim identity, source spans and checks.
        # Neither the old pass nor the old rejection can satisfy the new protocol.
        for version in ('before', 'after'):
            legacy = json.loads(record[version]['reader']['raw'])
            with self.assertRaises((KeyError, AssertionError)):
                g.focused.validate_judgment(legacy, self.packet)
        changed = copy.deepcopy(self.judgment)
        changed['claims'][0]['checks']['atomicity'] = dict(status='pass', reason='alternative synthetic reading')
        result = g.reconcile(self.packet, [self.judgment, changed, changed])
        self.assertEqual(result['status'], 'unresolved')
        self.assertTrue(result['differences'])
        self.assertIsNone(result['semantic_acceptance'])
        self.assertFalse(result['production_approval'])
        # Agreement cannot wash away a prior source/reader dispute or editorial contest.
        for flag in ('historical_disagreement', 'editorial_contested'):
            self.assertEqual(g.reconcile(self.packet, [changed, changed], **{flag: True})['status'], 'unresolved')

    def test_repeat_context_excludes_previous_verdict_and_preserves_source(self):
        q = g.quote(self.catalog, self.endpoints, 1000)
        first = g.payload(self.packet, q)
        second = g.payload(self.packet, q)
        self.assertEqual(first, second)
        content = json.loads(first['messages'][1]['content'])
        self.assertEqual(content['source']['source'], self.packet['case']['source'])
        self.assertEqual(content['candidate'], self.packet['candidate'])
        self.assertNotIn('structural_replay', content)
        protocol = first['messages'][0]['content']
        self.assertIn('cannot verify entailment', protocol)
        self.assertIn('not majority approval', protocol)
        self.assertIn('Unknown source identities', protocol)

    def test_one_repeat_only_identical_payload_and_separate_accounting(self):
        dispatcher = self.dispatcher()
        dispatcher.dispatch(self.packet)
        dispatcher.dispatch(self.packet, repeat=True)
        posts = [c[2] for c in self.calls if c[0] == 'POST']
        self.assertEqual(len(posts), 2)
        self.assertEqual(posts[0], posts[1])
        with self.connection() as conn:
            self.assertEqual(conn.execute('SELECT COUNT(*), SUM(actual) FROM request').fetchone(), (2, 2000000))
        with self.assertRaisesRegex(ValueError, 'already attempted'):
            dispatcher.dispatch(self.packet, repeat=True)

    def test_malformed_usage_retains_hold_and_raw_response(self):
        del self.response['usage']['cost']
        with self.assertRaises(KeyError):
            self.dispatcher().dispatch(self.packet)
        with self.connection() as conn:
            self.assertIsNone(conn.execute('SELECT actual FROM request').fetchone()[0])
        self.assertEqual(len(list((self.root / 'evidence').glob('*.response.json'))), 1)
        self.assertFalse(list((self.root / 'evidence').glob('*.judgment.json')))

    def test_actual_overrun_is_settled_and_stops_future_calls(self):
        self.response['usage']['cost'] = '0.2'
        with self.assertRaisesRegex(ValueError, 'price overrun'):
            self.dispatcher().dispatch(self.packet)
        with self.connection() as conn:
            self.assertEqual(conn.execute('SELECT actual FROM request').fetchone()[0], 200000000)
        self.assertFalse(list((self.root / 'evidence').glob('*.judgment.json')))
        calls = len(self.calls)
        with self.assertRaisesRegex(ValueError, 'budget blocked'):
            self.dispatcher().dispatch(g.packets()[1][1])
        self.assertEqual(len(self.calls), calls)
