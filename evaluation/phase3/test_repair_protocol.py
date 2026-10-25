"""Meaningful protocol regressions using a saved live review; no network."""
import copy
import io
import json
import sqlite3
import tempfile
import urllib.error
from pathlib import Path
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
        grouped_definitions=faceted_review.definitions('v47')
        self.assertEqual(len(grouped_definitions),3)
        fixed_order=repair.body(packet,'v48',active)['output_config']['format']['schema']['properties']['claims']['items']['properties']['dimensions']['properties']
        self.assertTrue(all(list(row['properties'])==['reason','status'] for row in fixed_order.values()))
        self.assertEqual(repair.body(packet,'v48',active)['max_tokens'],8000)
        self.assertEqual(repair.reader_body('One isolated Claim','v48')['max_tokens'],8000)
        self.assertEqual(repair.body(packet,'v49',active)['max_tokens'],8000)
        self.assertEqual(faceted_review.definitions('v49')[1],faceted_review.definitions('v48')[1])
        self.assertIn('COMPLETE scoped proposition',faceted_review.definitions('v49')[0]['question'])
        self.assertEqual({d for row in grouped_definitions for d in row['dimensions']},{d for row in faceted_review.definitions('v46') for d in row['dimensions']})
        joint=copy.deepcopy(violation_facet)
        for row in joint['claims']:row['dimensions']={d:dict(status='pass',reason='Synthetic whole-Claim support') for d in grouped_definitions[1]['dimensions']}
        self.assertEqual(repair.check(joint,packet,'v47',stage=active)['verdict'],'pass')
        missing=copy.deepcopy(joint);missing['claims'][0]['dimensions'].pop('korean_meaning')
        with self.assertRaises(ValueError):repair.check(missing,packet,'v47',stage=active)
        inconsistent=copy.deepcopy(joint);inconsistent['claims'][0]['dimensions']['source_meta_expansion']['status']='reject';inconsistent['verdict']='reject'
        with self.assertRaisesRegex(ValueError,'contradictory complete-Claim'):
            repair.check(inconsistent,packet,'v47',stage=active)
        scalar=copy.deepcopy(violation_facet)
        scalar['stage']=repair.stages('v45')['atomicity']
        for row in scalar['claims']:
            row['dimensions']={k:dict(status='pass',reason='Synthetic single independent assertion') for k in ['atomicity','duplicates']}
        scalar['inventory']=copy.deepcopy(canonical['inventory'])
        for item in scalar['inventory']:item['claim_index']=item.pop('claim_indices')[0]
        self.assertEqual(repair.check(scalar,packet,'v45',stage=scalar['stage'])['verdict'],'pass')
        for invalid_mapping in [None,[0],True]:
            invalid=copy.deepcopy(scalar);invalid['inventory'][0]['claim_index']=invalid_mapping
            with self.assertRaises(ValueError):repair.check(invalid,packet,'v45',stage=scalar['stage'])
        duplicate_review['claims'][0]['dimensions']['duplicates']['status']='reject'
        duplicate_review['verdict']='reject'
        self.assertEqual(repair.check(duplicate_review,packet,'v30')['verdict'],'reject')
        canonical['inventory'][0]['proposition']='invented interpretation'
        with self.assertRaisesRegex(ValueError,'canonical original inventory'):
            repair.check(canonical,packet,'v29')
        blind=repair.reader_body('ONE isolated Claim','v29')
        self.assertNotIn('original_source',blind['messages'][0]['content'])
        self.assertEqual(repair.reader_check(dict(verdict='pass',issues=[],checked=[0],coverage_complete=True))['verdict'],'pass')
        analyzed=dict(candidate_meaning='An isolated timeless assertion',
            analysis={k:dict(reason='Synthetic explicit interpretation',status='pass') for k in ['atomicity','reference','temporal']},
            verdict='pass',issues=[],checked=[0],coverage_complete=True)
        self.assertEqual(repair.reader_check(analyzed,'v50')['verdict'],'pass')
        self.assertEqual(repair.reader_check(analyzed,'v51')['verdict'],'pass')
        self.assertEqual(repair.reader_check(analyzed,'v52')['verdict'],'pass')
        self.assertEqual(repair.reader_check(analyzed,'v54')['verdict'],'pass')
        self.assertEqual(repair.body(packet,'v54',active),repair.body(packet,'v53',active))
        self.assertNotEqual(repair.reader_body('An isolated text','v54'),repair.reader_body('An isolated text','v53'))
        terminal=dict(case=dict(source='If the score stays low, the speaker plans to revise the model.'),candidate=packet['candidate'])
        stage52=repair.stages('v52')['atomicity']
        inventory52=repair.body(terminal,'v52',stage52)['output_config']['format']['schema']['properties']['inventory']['items']['properties']['proposition']['enum']
        inventory51=repair.body(terminal,'v51',stage52)['output_config']['format']['schema']['properties']['inventory']['items']['properties']['proposition']['enum']
        self.assertIn(terminal['case']['source'],inventory51)
        self.assertNotIn(terminal['case']['source'],inventory52)
        self.assertIn(terminal['case']['source'][:-1],inventory52)
        self.assertEqual(set(inventory51)-set(inventory52),{terminal['case']['source']})
        policy=(focused.ROOT/'prompts/extraction-v51/reference_policy.txt').read_text()
        for name in ['reader.txt','review.txt']:self.assertTrue((focused.ROOT/('prompts/extraction-v51/'+name)).read_text().endswith(policy))
        analyzed['analysis']['reference']['status']='reject'
        with self.assertRaisesRegex(ValueError,'contradictory standalone analysis'):repair.reader_check(analyzed,'v50')
        analyzed.update(verdict='reject',issues=['reference'])
        self.assertEqual(repair.reader_check(analyzed,'v50')['verdict'],'reject')
        analyzed['analysis']['temporal']['reason']=' '
        with self.assertRaisesRegex(ValueError,'blank standalone reason'):repair.reader_check(analyzed,'v50')
        with self.assertRaises(ValueError):repair.reader_check(dict(verdict='pass',issues=[],checked=[0],coverage_complete=True),'v50')
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
        with self.assertRaisesRegex(ValueError,'actual failed trace'):
            repair.extraction.attempt_label(recipe_packet,'extraction-v41','repair')
        linked=dict(recipe_packet,repair_context=dict(context,trace_sha256='a'*64))
        self.assertEqual(repair.extraction.attempt_label(linked,'extraction-v41','repair'),'extraction-v41-repair')
        self.assertEqual(repair.extraction.attempt_label(linked,'extraction-v41'),'extraction-v41')
        constrained=repair.extraction.body(recipe_packet,'extraction-v41','contract-v1')
        legacy=repair.extraction.body(recipe_packet,'extraction-v41')
        self.assertNotIn('format',legacy['output_config'])
        self.assertNotEqual(replay.request_identity(constrained),replay.request_identity(legacy))
        wire=constrained['output_config'].pop('format')
        self.assertEqual(constrained,legacy)
        self.assertEqual(wire['type'],'json_schema')
        ordered=repair.extraction.body(recipe_packet,'extraction-v43','grounded-v2')['output_config']['format']['schema']['properties']['claims']['items']
        self.assertEqual(list(ordered['properties'])[:2],['anchor','lang'])
        from faceted_review import exact_inventory_vocabulary
        spans=exact_inventory_vocabulary(recipe_packet['case']['source'])
        self.assertIn(recipe_packet['case']['source'],spans)
        self.assertTrue(all(recipe_packet['case']['source'].find(t)>=0 and recipe_packet['case']['source'].find(t,recipe_packet['case']['source'].find(t)+1)<0 for t in spans))
        self.assertLess(ordered['required'].index('text'),ordered['required'].index('mentions'))
        self.assertEqual(repair.extraction.attempt_label(linked,'extraction-v43',output_format='grounded-v2'),'extraction-v43-schema-v2')
        ordered_request=repair.extraction.body(recipe_packet,'extraction-v44','grounded-v3')
        ordered_bytes=repair.api.serialize_request(ordered_request,'ordered-json-v2')
        canonical_bytes=repair.api.serialize_request(ordered_request)
        self.assertNotEqual(ordered_bytes,canonical_bytes)
        receipt=dict(transport_serialization='ordered-json-v2',wire_sha256=focused.sha(ordered_bytes))
        repair.api.verify_request_serialization(receipt,ordered_request,'ordered-json-v2')
        reordered=focused.parse_judgment(canonical_bytes.decode())
        self.assertEqual(ordered_request,reordered)
        with self.assertRaisesRegex(ValueError,'byte order drift'):
            repair.api.verify_request_serialization(receipt,reordered,'ordered-json-v2')
        anchor=ordered_request['output_config']['format']['schema']['properties']['claims']['items']['properties']['anchor']['properties']
        self.assertEqual(anchor['occurrence']['enum'],[0])
        extended=repair.extraction.body(recipe_packet,'extraction-v45','grounded-v3',6000)
        self.assertEqual(extended['max_tokens'],6000)
        self.assertEqual(repair.extraction.attempt_label(linked,'extraction-v45','repair','grounded-v3',6000),'extraction-v45-schema-v3-output-6000-repair')
        self.assertTrue(all(t in recipe_packet['case']['source'] for t in anchor['quote']['enum']))
        self.assertEqual(set(wire['schema']['properties']),{'claims','empty_reason'})
        self.assertEqual(set(wire['schema']['properties']['claims']['items']['required']),set(recipe_packet['candidate']['claims'][0]))
        self.assertFalse(wire['schema']['additionalProperties'])
        self.assertEqual(repair.extraction.attempt_label(linked,'extraction-v41','repair','contract-v1'),'extraction-v41-schema-v1-repair')
        with self.assertRaisesRegex(ValueError,'unknown extraction output format'):
            repair.extraction.attempt_label(linked,'extraction-v41',output_format='guess')
        # Offline transport failure must retain its reservation and remain replayable.
        with tempfile.TemporaryDirectory() as directory:
            ledger=Path(directory)/'ledger.sqlite'
            evidence=Path(directory)/'evidence'
            conn=sqlite3.connect(ledger)
            conn.executescript("CREATE TABLE budget(id INTEGER PRIMARY KEY,ceiling INTEGER,prior INTEGER); CREATE TABLE request(id TEXT PRIMARY KEY,model TEXT,created_at TEXT,estimate INTEGER,actual INTEGER,provider_id TEXT,settled_at TEXT,cumulative INTEGER,prompt_tokens INTEGER,completion_tokens INTEGER);")
            conn.execute('INSERT INTO budget VALUES(1,?,0)',(repair.budget.LIMIT,))
            conn.commit();conn.close()
            def failed_transport(method,path,body):
                if method=='GET':return dict(id=repair.api.MODEL)
                raise urllib.error.HTTPError('https://invalid.test',400,'schema error',{},io.BytesIO(b'fixture-secret schema failure'))
            authorization=dict(provider='anthropic_direct',explicit_user_authorization=True,
                additional_limit_nanos=repair.budget.CONTINUATION,strict_total_below_nanos=repair.budget.STRICT_TOTAL,
                authorization_reference='offline fixture only')
            with mock.patch.object(repair.budget,'LEDGER',ledger), mock.patch.object(repair.api,'transport',return_value=failed_transport), mock.patch.dict('os.environ',{'ANTHROPIC_API_KEY':'fixture-secret'}):
                with self.assertRaisesRegex(ValueError,'reservation retained'):
                    repair.extraction.dispatch(recipe_packet,evidence,authorization,'extraction-v41',output_format='contract-v1')
                held=replay.replay(evidence)
            self.assertEqual(held['extraction_requests'],1)
            self.assertEqual(held['ledger']['unsettled'],1)
            self.assertTrue(held['rows'][0]['pending_reservation'])
            self.assertFalse(held['rows'][0]['model_response'])
            self.assertNotIn('fixture-secret',held['rows'][0]['transport_error']['response_body'])
            self.assertEqual(held['rows'][0]['transport_error']['status'],400)
            self.assertGreater(held['ledger']['recorded_upper_nanos'],0)

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
        # An actual repeated rejection remains a distinct frozen request, never a vote.
        repeated=complete.replay(root,'v23-fresh-003','extraction-v48',
            'extraction-v44-schema-v3',source_replicates={'fidelity':'repeat'})
        self.assertFalse(repeated['revalidation']['experimental_gate_eligible'])
        self.assertEqual(repeated['revalidation']['review_error'],'semantic')
        self.assertIn('-repeat-schema-v3-primary',repeated['receipts'][-1]['id'])
        with self.assertRaisesRegex(ValueError,'unsupported Source replicate'):
            complete.replay(root,'v23-fresh-003','extraction-v48',
                'extraction-v44-schema-v3',source_replicates={'atomicity':'invented'})
        # Reuse is conditional on exact recipes, including supposedly unchanged components.
        with mock.patch.object(complete.extraction,'body',return_value={}):
            with self.assertRaisesRegex(ValueError,'extraction request drift'):
                complete.replay(root,'r1-nested_quote','extraction-v34','extraction-v33','v33',6000)
        with mock.patch.object(complete.review,'reader_body',return_value={}):
            with self.assertRaisesRegex(ValueError,'reader request drift'):
                complete.replay(root,'r1-nested_quote','extraction-v34','extraction-v33','v33',6000)
