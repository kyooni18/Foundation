"""Meaningful protocol regressions using a saved live review; no network."""
import copy
import unittest
from unittest import mock

import replay_complete_semantic_trace as complete

import focused_review as focused
import guarded_anthropic_repair as repair
import replay_semantic_repair as replay


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
        self.assertEqual(repair.check(duplicate_review,packet,'v32')['verdict'],'pass')
        self.assertEqual(repair.check(duplicate_review,packet,'v33')['verdict'],'pass')
        self.assertEqual(repair.check(duplicate_review,packet,'v34')['verdict'],'pass')
        self.assertEqual(repair.check(duplicate_review,packet,'v35')['verdict'],'pass')
        self.assertEqual(repair.body(packet,'v35')['max_tokens'],6000)
        self.assertEqual(repair.check(duplicate_review,packet,'v36')['verdict'],'pass')
        self.assertEqual(repair.body(packet,'v36')['max_tokens'],6000)
        self.assertEqual(repair.check(duplicate_review,packet,'v37')['verdict'],'pass')
        self.assertEqual(repair.body(packet,'v37')['max_tokens'],6000)
        korean_rule=repair.body(packet,'v38')['output_config']['format']['schema']['properties']['claims']['items']['properties']['dimensions']['properties']['korean_meaning']['properties']['status']
        self.assertNotIn('not_applicable',korean_rule['enum'])
        english_packet=dict(case=dict(source='A service runs.'),candidate=dict(claims=[dict(text='A service runs.',lang='en')]))
        english_rule=repair.body(english_packet,'v38')['output_config']['format']['schema']['properties']['claims']['items']['properties']['dimensions']['properties']['korean_meaning']['properties']['status']
        self.assertIn('not_applicable',english_rule['enum'])
        for component in ['frames.txt','reader.txt']:
            self.assertEqual((focused.ROOT/'prompts/extraction-v35'/component).read_bytes(),
                             (focused.ROOT/'prompts/extraction-v36'/component).read_bytes())
        source_root=focused.ROOT/'prompts'
        for component in ['frames.txt','reader.txt']:
            self.assertEqual((source_root/'extraction-v33'/component).read_bytes(),
                             (source_root/'extraction-v34'/component).read_bytes())
        extended=repair.body(packet,'v34',output_tokens=6000)
        regular=repair.body(packet,'v34')
        self.assertNotEqual(replay.request_identity(extended),replay.request_identity(regular))
        extended['max_tokens']=3000
        self.assertEqual(extended,regular)
        with self.assertRaisesRegex(ValueError,'unsupported review response budget'):
            repair.body(packet,'v33',output_tokens=6000)
        contradiction=copy.deepcopy(duplicate_review)
        contradiction['claims'][0]['dimensions']['unsupported_inference']['status']='reject'
        contradiction['verdict']='reject'
        with self.assertRaisesRegex(ValueError,'contradictory complete-claim entailment'):
            repair.check(contradiction,packet,'v34')
        self.assertEqual(repair.check(contradiction,packet,'v34',strict_entailment=False)['verdict'],'reject')
        shared=copy.deepcopy(duplicate_review)
        shared['inventory'][0]['claim_indices']=[0,1]
        with self.assertRaisesRegex(ValueError,'one-to-one'):
            repair.check(shared,packet,'v38')
        self.assertEqual(repair.check(shared,packet,'v38',strict_bijection=False)['verdict'],'pass')
        shared=copy.deepcopy(duplicate_review)
        shared['inventory'][1]['proposition']=shared['inventory'][0]['proposition']
        with self.assertRaisesRegex(ValueError,'one-to-one'):
            repair.check(shared,packet,'v38')
        import faceted_review
        active=repair.stages('v39')['fidelity']
        structured=repair.body(packet,'v39',active)
        self.assertEqual(structured['max_tokens'],6000)
        self.assertEqual(structured['system'],faceted_review.prompt(active))
        self.assertEqual(len(repair.stages('v39')),9)
        self.assertEqual(repair.stages('v40'),repair.stages('v39'))
        for key,stage_name in repair.stages('v40').items():
            a=repair.body(packet,'v39',stage_name)
            b=repair.body(packet,'v40',stage_name)
            if key in {'atomicity','source_meta','coverage'}:self.assertNotEqual(a,b)
            else:self.assertEqual(a,b)
        self.assertEqual(repair.reader_body('One isolated proposition','v39'),repair.reader_body('One isolated proposition','v40'))
        def supported_arrays(value):
            if isinstance(value,dict):
                self.assertNotIn('maxItems',value)
                self.assertIn(value.get('minItems',0),[0,1])
                for child in value.values():supported_arrays(child)
            elif isinstance(value,list):
                for child in value:supported_arrays(child)
        supported_arrays(structured['output_config']['format']['schema'])
        facet=dict(stage=active,verdict='pass',uncertainty_flags=[],inventory=[],claims=[dict(index=i,text=c['text'],evidence=[dict(quote=source)],source_meaning=c['text'],candidate_meaning=c['text'],dimensions=dict(entailment=dict(status='pass',reason='Synthetic exact support')),differences=[],uncertainty_flags=[]) for i,c in enumerate(packet['candidate']['claims'])])
        self.assertEqual(repair.check(facet,packet,'v39',stage=active)['verdict'],'pass')
        with self.assertRaisesRegex(ValueError,'responsibility binding'):
            repair.check(facet,packet,'v39')
        changed=copy.deepcopy(facet)
        changed['claims'][0]['differences']=['Unsupported added grammar commentary']
        with self.assertRaisesRegex(ValueError,'contradicts reported differences'):
            repair.check(changed,packet,'v39',stage=active)
        violation_facet=copy.deepcopy(facet)
        for c in violation_facet['claims']:c['violations']=c.pop('differences')
        self.assertEqual(repair.check(violation_facet,packet,'v41',stage=active)['verdict'],'pass')
        with self.assertRaises(ValueError):repair.check(facet,packet,'v41',stage=active)
        protected=repair.body(packet,'v41',active)['output_config']['format']['schema']['properties']['claims']['items']['properties']
        self.assertIn('pattern',protected['source_meaning'])
        self.assertIn('pattern',protected['dimensions']['properties']['entailment']['properties']['reason'])
        self.assertIn('violations',protected)
        self.assertNotIn('differences',protected)
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
        # A real v30 response rejected without inspecting either supplied Claim.
        failed='focused-haiku-repair-v30-r1-nested_quote-extraction-v30-primary'
        saved=focused.parse_judgment((root/(failed+'.request.json')).read_text())
        payload=focused.parse_judgment(saved['payload']['messages'][0]['content'])
        packet=dict(case=dict(source=payload['original_source']),candidate=payload['candidate_batch'])
        response=focused.parse_judgment((root/(failed+'.response.json')).read_text())
        raw=''.join(b['text'] for b in response['content'] if b.get('type')=='text')
        with self.assertRaisesRegex(ValueError,'review omitted Claim'):
            repair.check(focused.parse_judgment(raw),packet,'v30')
        constrained=repair.body(packet,'v31')['output_config']['format']['schema']['properties']
        self.assertEqual(constrained['claims']['minItems'],1)
        self.assertEqual(constrained['inventory']['minItems'],1)
        empty_packet['candidate']['empty_reason']='No assertion.'
        unconstrained=repair.body(empty_packet,'v31')['output_config']['format']['schema']['properties']
        self.assertNotIn('minItems',unconstrained['claims'])
        low=repair.body(packet,'v31')
        medium=repair.body(packet,'v31',effort='medium')
        self.assertNotEqual(replay.request_identity(low),replay.request_identity(medium))
        self.assertEqual(low['max_tokens'],medium['max_tokens'])
        medium['output_config']['effort']='low'
        self.assertEqual(low,medium)
        for key,value in [('model','another-model'),('max_tokens',2999),('service_tier','auto')]:
            changed=copy.deepcopy(low)
            changed[key]=value
            self.assertNotEqual(replay.request_identity(low),replay.request_identity(changed))
        with self.assertRaisesRegex(ValueError,'unsupported diagnostic effort'):
            repair.body(packet,'v30',effort='medium')
        case=next(p['case'] for p in repair.budget.packets()[1] if p['case']['id']=='r1-nested_quote')
        recipe_packet=next(p for p in repair.budget.packets()[1] if p['case']['id']=='r1-nested_quote')
        context=dict(source_sha256=focused.sha(recipe_packet['case']['source'].encode()),failure_code='semantic',
            candidate_batch=recipe_packet['candidate'],review_reports=[],authority='untrusted model output')
        recipe=repair.extraction.body(dict(recipe_packet,repair_context=context),'extraction-v38')
        payload=focused.parse_judgment(recipe['messages'][0]['content'])
        self.assertEqual(payload['prior_extraction_claims'],context['candidate_batch']['claims'])
        self.assertEqual(payload['previous_failure_code'],'semantic')
        self.assertEqual(payload['untrusted_previous_attempt'],context)
        with self.assertRaisesRegex(ValueError,'repair Source drift'):
            repair.extraction.body(dict(recipe_packet,repair_context=dict(context,source_sha256='wrong')),'extraction-v38')
        invalid=focused.parse_judgment((root/'focused-haiku-extraction-v32-r1-nested_quote-primary.candidate.json').read_text())
        self.assertFalse(repair.extraction.rust_structure(invalid,case)['valid'])
        # A private fixture isolates the metadata defect; raw model evidence is immutable.
        fixed=copy.deepcopy(invalid)
        fixed['claims'][1]['index_fields']=None
        self.assertTrue(repair.extraction.rust_structure(fixed,case)['valid'])

        for suffix in ['output-6000','repeat-output-6000']:
            ident='focused-haiku-repair-v34-r1-nested_quote-'+suffix+'-primary'
            saved=focused.parse_judgment((root/(ident+'.request.json')).read_text())
            payload=focused.parse_judgment(saved['payload']['messages'][0]['content'])
            negative=focused.parse_judgment((root/(ident+'.judgment.json')).read_text())
            with self.assertRaisesRegex(ValueError,'contradictory complete-claim entailment'):
                repair.check(negative,dict(case=dict(source=payload['original_source']),candidate=payload['candidate_batch']),'v34')
        trace=complete.replay(root,'r1-nested_quote','extraction-v34','extraction-v33','v33',6000)
        self.assertTrue(trace['revalidation']['experimental_gate_eligible'])
        self.assertTrue(trace['complete_review_count'])
        # Reuse is conditional on exact recipes, including supposedly unchanged components.
        with mock.patch.object(complete.extraction,'body',return_value={}):
            with self.assertRaisesRegex(ValueError,'extraction request drift'):
                complete.replay(root,'r1-nested_quote','extraction-v34','extraction-v33','v33',6000)
        with mock.patch.object(complete.review,'reader_body',return_value={}):
            with self.assertRaisesRegex(ValueError,'reader request drift'):
                complete.replay(root,'r1-nested_quote','extraction-v34','extraction-v33','v33',6000)
