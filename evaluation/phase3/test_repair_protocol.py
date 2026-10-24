"""Meaningful protocol regressions using a saved live review; no network."""
import copy
import unittest

import focused_review as focused
import guarded_anthropic_repair as repair


class RepairProtocolTest(unittest.TestCase):
    def test_saved_control_and_malformed_review_fail_closed(self):
        root=focused.ROOT/'reports/evidence/phase-3/semantic-repair'
        ident='focused-haiku-repair-v28-r1-nested_quote-control-nested-perspective-primary'
        request=focused.parse_judgment((root/(ident+'.request.json')).read_text())
        payload=focused.parse_judgment(request['payload']['messages'][0]['content'])
        packet=dict(case=dict(source=payload['original_source']),candidate=payload['candidate_batch'])
        judgment=focused.parse_judgment((root/(ident+'.judgment.json')).read_text())
        self.assertEqual(repair.check(judgment,packet,'v28')['verdict'],'pass')
        mutations=[
            lambda j:j['uncertainty_flags'].append('contested source interpretation'),
            lambda j:j['claims'][0]['dimensions'].pop('source_meta_expansion'),
            lambda j:j['inventory'][0]['claim_indices'].append(j['inventory'][0]['claim_indices'][0]),
            lambda j:j['claims'][0].update(index=False),
            lambda j:j['inventory'][1].update(claim_indices=[0]),
            lambda j:j['claims'][0]['dimensions']['korean_meaning'].update(status='not_applicable'),
        ]
        for mutate in mutations:
            with self.subTest(mutation=mutations.index(mutate)):
                changed=copy.deepcopy(judgment)
                mutate(changed)
                with self.assertRaises(ValueError):
                    repair.check(changed,packet,'v28')
        canonical=copy.deepcopy(judgment)
        source=packet['case']['source']
        for row in canonical['claims']:
            row['evidence']=[dict(quote=source)]
        for row,text in zip(canonical['inventory'],source.split('. ')):
            row.update(proposition=text.rstrip('.'),evidence=[dict(quote=source)])
        self.assertEqual(repair.check(canonical,packet,'v29')['verdict'],'pass')
        canonical['inventory'][0]['proposition']='invented interpretation'
        with self.assertRaisesRegex(ValueError,'canonical original inventory'):
            repair.check(canonical,packet,'v29')
        blind=repair.reader_body('ONE isolated Claim','v29')
        self.assertNotIn('original_source',blind['messages'][0]['content'])
        self.assertEqual(repair.reader_check(dict(verdict='pass',issues=[],checked=[0],coverage_complete=True))['verdict'],'pass')
        with self.assertRaises(ValueError):
            repair.reader_check(dict(verdict='pass',issues=['reference'],checked=[0],coverage_complete=True))
        request_text=repair.body(packet,'v28')['messages'][0]['content']
        self.assertNotIn('expected_verdict',request_text)
        self.assertNotIn('authorship',request_text)
