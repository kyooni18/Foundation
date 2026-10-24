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
            lambda j:j['inventory'].append(dict(j['inventory'][0],reason='a different explanation of the same proposition')),
            lambda j:j['claims'][0].update(index=False),
            lambda j:j['inventory'][1].update(claim_indices=[0]),
            lambda j:j['claims'][0]['dimensions']['korean_meaning'].update(status='not_applicable'),
            lambda j:j['claims'][0]['dimensions']['atomicity'].update(reason=' \t\n'),
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
        with self.assertRaises(ValueError):
            repair.check(canonical,packet,'v30')
        duplicate_review=copy.deepcopy(canonical)
        for row in duplicate_review['claims']:
            row['dimensions']['duplicates']=dict(status='pass',reason='independent source assertions')
        self.assertEqual(repair.check(duplicate_review,packet,'v30')['verdict'],'pass')
        duplicate_review['claims'][0]['dimensions']['duplicates']['status']='reject'
        duplicate_review['verdict']='reject'
        self.assertEqual(repair.check(duplicate_review,packet,'v30')['verdict'],'reject')
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
        empty_packet=dict(case=dict(source='...'),candidate=dict(claims=[],empty_reason='No independently asserted proposition.'))
        no_propositions=dict(verdict='pass',uncertainty_flags=[],claims=[],inventory=[])
        self.assertEqual(repair.check(no_propositions,empty_packet,'v30')['verdict'],'pass')
        empty_packet['candidate']['empty_reason']=None
        with self.assertRaises(ValueError):
            repair.check(no_propositions,empty_packet,'v30')
        decomposed='\u1100\u1161. Four workers are configured.'
        english='Four workers are configured.'
        decomposed_packet=dict(case=dict(source=decomposed),candidate=dict(claims=[dict(text=english,lang='en')],empty_reason=None))
        na_dimensions=copy.deepcopy(judgment['claims'][0]['dimensions'])
        na_dimensions['korean_meaning']['status']='not_applicable'
        decomposed_review=dict(verdict='pass',uncertainty_flags=[],claims=[dict(index=0,text=english,
            evidence=[dict(quote=decomposed)],dimensions=na_dimensions,uncertainty_flags=[])],
            inventory=[dict(proposition=english,evidence=[dict(quote=decomposed)],claim_indices=[0],status='pass',reason='represented')])
        with self.assertRaisesRegex(ValueError,'unassessed Korean dependency'):
            repair.check(decomposed_review,decomposed_packet,'v28')
