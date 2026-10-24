"""Offline tests: never use an API key or network."""
from contextlib import contextmanager
from datetime import date,timedelta
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

import guarded_anthropic as a
import guarded_haiku as g


class DirectClaudeTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.ledger = self.root / 'budget.sqlite'
        with self.connection() as conn:
            conn.executescript('CREATE TABLE budget(id INTEGER PRIMARY KEY, ceiling INTEGER, prior INTEGER);'
                'INSERT INTO budget VALUES(1,4882653350,4782653350);'
                'CREATE TABLE request(id TEXT PRIMARY KEY,model TEXT,created_at TEXT,estimate INTEGER,'
                'actual INTEGER,provider_id TEXT,settled_at TEXT,cumulative INTEGER,prompt_tokens INTEGER,completion_tokens INTEGER);')
        self.packet = g.packets()[1][0]
        self.calls = []
        source = self.packet['case']['source']
        evidence = [dict(start=0,end=len(source),quote=source)]
        self.judgment = dict(verdict='unresolved',uncertainty_flags=['synthetic fixture'],
            claims=[dict(index=i,text=c['text'],evidence=evidence,uncertainty_flags=[],
                    checks={k:dict(status='unresolved',reason='synthetic fixture') for k in g.focused.CHECKS})
                    for i,c in enumerate(self.packet['candidate']['claims'])],
            inventory=[dict(reference_index=i,status='unresolved',claim_indices=[],
                            evidence=evidence,reason='synthetic fixture')
                       for i,_ in enumerate(self.packet['case']['reference']['propositions'])])
        self.response = dict(type='message',id='msg_fixture',model=a.MODEL,stop_reason='end_turn',
            usage=dict(input_tokens=600,output_tokens=250,cache_creation_input_tokens=0,
                       cache_read_input_tokens=0),
            content=[dict(type='text',text=json.dumps(self.judgment,ensure_ascii=False))])
        self.authorization = dict(explicit_user_authorization=True,
            additional_limit_nanos=g.CONTINUATION,strict_total_below_nanos=5000000000,
            authorization_reference='synthetic unit test only', provider='anthropic_direct')

    @contextmanager
    def connection(self):
        conn = sqlite3.connect(self.ledger)
        try:
            with conn:
                yield conn
        finally:
            conn.close()

    def transport(self,method,path,body):
        self.calls.append((method,path,body))
        if method == 'GET':
            return dict(id=a.MODEL,type='model')
        with self.connection() as conn:
            self.assertEqual(conn.execute('SELECT COUNT(*) FROM request WHERE actual IS NULL').fetchone()[0],1)
        return self.response

    def dispatcher(self):
        return a.Dispatcher(self.ledger,self.root/'evidence',self.transport,self.authorization)

    def test_structured_request_budget_and_conservative_usage(self):
        body=a.request_body(self.packet)
        self.assertEqual(body['model'],a.MODEL)
        self.assertEqual(body['output_config']['effort'],'low')
        self.assertEqual(body['output_config']['format']['type'],'json_schema')
        self.assertTrue(set(g.focused.CHECKS) <= set(body['output_config']['format']['schema']
            ['properties']['claims']['items']['properties']['checks']['properties']))
        self.assertEqual(a.quote(body,today=a.PRICE_CHECK_DATE)['charge_type'],
                         'conservative_upper_bound_not_provider_billed_cost')
        result=self.dispatcher().dispatch(self.packet)
        self.assertEqual(result,self.judgment)
        with self.connection() as conn:
            row=conn.execute('SELECT model,estimate,actual,prompt_tokens,completion_tokens FROM request').fetchone()
            self.assertEqual(row[0],'anthropic/'+a.MODEL)
            self.assertLess(row[2],row[1])
            self.assertEqual(row[2],a.conservative_usage(self.response)['conservative_upper_nanos'])
            self.assertEqual(row[3:],(600,250))
        receipt=json.loads(next((self.root/'evidence').glob('*.accounting.json')).read_text())
        self.assertIsNone(receipt['billed_cost_usd'])
        self.assertFalse('OPENROUTER_API_KEY' in str(receipt))
        with self.assertRaisesRegex(ValueError,'already attempted'):
            self.dispatcher().dispatch(self.packet)
        self.assertEqual([x[0] for x in self.calls],['GET','POST'])

    def test_reservation_uses_request_cap_without_double_padding(self):
        body=a.request_body(self.packet)
        body['max_tokens']=6000
        quote=a.quote(body,today=a.PRICE_CHECK_DATE)
        size=len(g.focused.canonical(body))+a.OVERHEAD_TOKENS
        inp,out=a.rates(size)
        expected=g.nanos((size*inp+6000*out)/1_000_000)
        old=g.nanos(2*(size*inp+8000*out)/1_000_000)
        self.assertEqual(quote['estimate'],expected)
        self.assertLess(quote['estimate'],old//2)
        self.assertEqual(quote['output_token_bound'],body['max_tokens'])
        self.assertEqual(quote['input_byte_bound'],size)
        worst=a.conservative_usage(dict(usage=dict(input_tokens=size,output_tokens=6000)))
        self.assertGreaterEqual(quote['estimate'],worst['conservative_upper_nanos'])
        for bad in (None,False,0,-1,6000.0,'6000',a.MAX_OUTPUT_TOKENS+1):
            with self.subTest(output_bound=bad):
                body['max_tokens']=bad
                with self.assertRaisesRegex(ValueError,'invalid request output bound'):
                    a.quote(body,today=a.PRICE_CHECK_DATE)

    def test_budget_blocks_before_any_provider_request(self):
        with self.connection() as conn:
            conn.execute('UPDATE budget SET prior=4882653350')
        before=self.ledger.read_bytes()
        with self.assertRaisesRegex(ValueError,'budget blocked'):
            self.dispatcher().dispatch(self.packet)
        self.assertFalse(self.calls)
        self.assertEqual(self.ledger.read_bytes(),before)

    def test_model_identity_failure_leaves_no_reservation(self):
        def wrong(method,path,body):
            self.calls.append((method,path,body))
            return dict(id='unexpected-model')
        with self.assertRaisesRegex(ValueError,'unexpected model identity'):
            a.Dispatcher(self.ledger,self.root/'evidence',wrong,self.authorization).dispatch(self.packet)
        self.assertEqual(len(self.calls),1)
        with self.connection() as conn:
            self.assertEqual(conn.execute('SELECT COUNT(*) FROM request').fetchone()[0],0)

    def test_timeout_retains_hold_and_does_not_retry(self):
        def timeout(method,path,body):
            if method == 'POST':
                raise TimeoutError('synthetic network timeout')
            return self.transport(method,path,body)
        dispatcher=a.Dispatcher(self.ledger,self.root/'evidence',timeout,self.authorization)
        with self.assertRaises(TimeoutError):
            dispatcher.dispatch(self.packet)
        with self.connection() as conn:
            self.assertIsNone(conn.execute('SELECT actual FROM request').fetchone()[0])
        with self.assertRaisesRegex(ValueError,'already attempted'):
            dispatcher.dispatch(self.packet)

    def test_invalid_json_is_accounted_but_never_approved(self):
        self.response['content'][0]['text']='{"verdict":"pass","verdict":"fail"}'
        with self.assertRaisesRegex(ValueError,'duplicate judgment member'):
            self.dispatcher().dispatch(self.packet)
        self.assertFalse(list((self.root/'evidence').glob('*.judgment.json')))
        with self.connection() as conn:
            self.assertIsNotNone(conn.execute('SELECT actual FROM request').fetchone()[0])

    def test_unknown_usage_cost_component_keeps_uncertain_reservation(self):
        self.response['usage']['unexpected_charge_tokens']=1
        with self.assertRaisesRegex(ValueError,'unknown usage component'):
            self.dispatcher().dispatch(self.packet)
        with self.connection() as conn:
            self.assertIsNone(conn.execute('SELECT actual FROM request').fetchone()[0])
        self.assertEqual(len(list((self.root/'evidence').glob('*.response.json'))),1)

    def test_thinking_block_and_documented_usage_fields_are_accepted(self):
        self.response['content'].insert(0,dict(type='thinking',thinking='',signature='sig'))
        self.response['usage'].update(output_tokens_details=dict(thinking_tokens=100),
            service_tier='standard',speed='standard',inference_geo='global')
        self.assertEqual(self.dispatcher().dispatch(self.packet),self.judgment)
        self.assertTrue((self.root/'evidence'/('focused-haiku-%s-primary-%s.judgment.json'
                                               % (self.packet['case']['id'],a.ATTEMPT))).is_file())

    def test_truncated_response_is_settled_but_never_judged(self):
        self.response['stop_reason']='max_tokens'
        with self.assertRaisesRegex(ValueError,'incomplete model response'):
            self.dispatcher().dispatch(self.packet)
        self.assertFalse(list((self.root/'evidence').glob('*.judgment.json')))
        with self.connection() as conn:
            self.assertIsNotNone(conn.execute('SELECT actual FROM request').fetchone()[0])

    def test_miscounted_offsets_reanchor_only_unique_exact_quotes(self):
        source='가 나 가'
        j=dict(claims=[dict(index=0,evidence=[dict(start=1,end=3,quote='나'),
                                               dict(start=0,end=1,quote='가'),
                                               dict(start=2,end=3,quote='가')])],inventory=[])
        fixed,corrections=a.anchor_evidence(j,source)
        self.assertEqual(fixed['claims'][0]['evidence'][0],dict(start=2,end=3,quote='나'))
        self.assertEqual(fixed['claims'][0]['evidence'][2],dict(start=2,end=3,quote='가'))
        self.assertEqual(len(corrections),1)
        self.assertEqual(j['claims'][0]['evidence'][0]['start'],1)
        source=self.packet['case']['source']
        for c in self.judgment['claims']:
            c['evidence']=[dict(start=1,end=len(source)+5,quote=source)]
        self.response['content'][0]['text']=json.dumps(self.judgment,ensure_ascii=False)
        self.dispatcher().dispatch(self.packet)
        self.assertEqual(len(list((self.root/'evidence').glob('*.anchoring.json'))),1)

    def test_unicode_reanchoring_never_guesses_repeated_or_normalized_quotes(self):
        source='👩🏽‍💻 가나다 e\u0301 “안녕” 가나다'
        quote='“안녕”'
        evidence=[dict(start=source.index(quote)+3,end=source.index(quote)+3+len(quote),quote=quote),
                  dict(start=0,end=1,quote='가나다'),
                  dict(start=0,end=1,quote='é')]
        judged, corrections=a.anchor_evidence(dict(claims=[dict(index=0,evidence=evidence)],inventory=[]),source)
        self.assertEqual(len(corrections),1)
        self.assertEqual(judged['claims'][0]['evidence'][0]['start'],source.index(quote))
        self.assertEqual(judged['claims'][0]['evidence'][1],evidence[1])
        self.assertEqual(judged['claims'][0]['evidence'][2],evidence[2])
        self.assertEqual(source[judged['claims'][0]['evidence'][0]['start']:judged['claims'][0]['evidence'][0]['end']],quote)

    def test_pass_verdict_with_uncertainty_is_recorded_not_coerced(self):
        self.judgment['verdict']='pass'
        for c in self.judgment['claims']:
            c['checks']={k:dict(status='pass',reason='synthetic') for k in g.focused.CHECKS}
        for r in self.judgment['inventory']:
            r.update(status='pass',claim_indices=[0])
        self.response['content'][0]['text']=json.dumps(self.judgment,ensure_ascii=False)
        with self.assertRaisesRegex(ValueError,'protocol-invalid'):
            self.dispatcher().dispatch(self.packet)
        rejection=json.loads(next((self.root/'evidence').glob('*.rejection.json')).read_text())
        self.assertEqual((rejection['model_verdict'],rejection['semantic_acceptance']),('pass',None))
        self.assertFalse(list((self.root/'evidence').glob('*.judgment.json')))

    def test_long_prompt_tier_and_us_inference_are_not_underpriced(self):
        small=a.conservative_usage(dict(usage=dict(input_tokens=100000,output_tokens=0)))
        large=a.conservative_usage(dict(usage=dict(input_tokens=100001,output_tokens=0)))
        self.assertEqual(small['conservative_upper_nanos'],20000000)
        self.assertEqual(large['conservative_upper_nanos'],100001000)
        with self.assertRaisesRegex(ValueError,'unpriced service tier'):
            a.conservative_usage(dict(usage=dict(input_tokens=1,output_tokens=1,inference_geo='us')))

    def test_pricing_expiry_and_missing_budget_authorization(self):
        with self.assertRaisesRegex(ValueError,'pricing must be re-verified'):
            a.quote(a.request_body(self.packet),today=a.PRICE_CHECK_DATE+timedelta(days=8))
        with self.assertRaisesRegex(ValueError,'provider authorization missing'):
            a.Dispatcher(self.ledger,self.root/'evidence',self.transport,{})
        with self.assertRaisesRegex(ValueError,'provider authorization missing'):
            a.Dispatcher(self.ledger,self.root/'evidence',self.transport,{
                k:v for k,v in self.authorization.items() if k != 'provider'})
        with self.connection() as conn:
            self.assertEqual(conn.execute('SELECT COUNT(*) FROM request').fetchone()[0],0)


if __name__=='__main__':
    unittest.main()
