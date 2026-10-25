"""Offline current checkpoint audit; no provider request or approval."""
import json,sqlite3,subprocess,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'evaluation/phase3'))
import focused_review as f,guarded_haiku as g
P=Path('reports/evidence/phase-3/resumed-haiku')
c=sqlite3.connect(g.LEDGER)
old=json.loads((P/'ledger-before.json').read_text())
current={r[0]:list(r) for r in c.execute('SELECT * FROM request')}
g.require(all(current[r[0]]==r for r in old['requests']),'historical charge changed')
budget=c.execute('SELECT ceiling,prior FROM budget').fetchone()
g.require(budget==tuple(old['budget'][0][1:]),'budget changed')
recorded=c.execute('SELECT SUM(COALESCE(actual,estimate)) FROM request').fetchone()[0]
unknown=[dict(id=r[0],reservation_nanos=r[1]) for r in c.execute('SELECT id,estimate FROM request WHERE actual IS NULL ORDER BY id')]
g.require(len(unknown)==4,'new unresolved transport or active dispatch; audit only terminal calls')
verification=json.loads((P/'verification-v54/results.json').read_text())
g.require(all(r['exit_code']==0 for r in verification),'required check failed')
selection=[(case,f'v54-{case}-trace.json') for case in ['r1-ambiguous_ref','v23-fresh-003','r1-hypothetical','r1-nested_quote','v23-fresh-004','r1-habitual_temporal']]
rows=[]
for case,name in selection:
 t=json.loads((P/name).read_text())
 rows.append(dict(case=case,trace=name,prompt=t['replay_row']['plan']['prompt_version'],claims=t['claim_count'],complete_reviews=t['complete_review_count'],revalidation=t['revalidation'],trace_sha256=f.sha((P/name).read_bytes()),independent_gold=False,editorial_withhold=False,production_approval=t['production_approval']))
negative={}
for version in ['v48','v49']:
 rs=json.loads((P/(version+'-negative-results.json')).read_text());negative[version]=dict(calls=len(rs),known_candidates=4,coherent_rejects=sum(r['exit_code']==0 and json.loads(r['stdout']).get('protocol_valid') is True and json.loads(r['stdout']).get('verdict')=='reject' for r in rs))
boundary=json.loads((P/'v54-final-boundary-results.json').read_text())
g.require(len(boundary)==6 and [json.loads(r['stdout'])['verdict'] for r in boundary]==['reject','reject','reject','pass','pass','pass'],'current failed-prefix or repeat evidence disagrees')
reference=json.loads((P/'v54-reference-probe.json').read_text())
g.require(len(reference)==3 and [json.loads(r['stdout'])['verdict'] for r in reference]==['pass','reject','reject'],'reference control evidence failed')
E=Path('reports/evidence/phase-3/semantic-repair');repeat_checks=[]
primary_versions={'r1-nested_quote':'v54','v23-fresh-003':'v52','r1-hypothetical':'v53'}
for run in boundary[3:]:
 g.require(run['exit_code']==0,'repeat execution failed');outcome=json.loads(run['stdout']);ident=outcome['id'];repeated=json.loads((E/(ident+'.request.json')).read_text())
 case=run['command'][run['command'].index('--case')+1]
 primary_id=ident.replace('-repeat-reader-','-reader-').replace('-repeat-schema-v3-primary','-schema-v3-primary').replace('repair-v54-','repair-'+primary_versions[case]+'-')
 primary=json.loads((E/(primary_id+'.request.json')).read_text());g.require(primary['payload']==repeated['payload'] and primary['wire_sha256']==repeated['wire_sha256'],'repeat request drift')
 repeat_checks.append(dict(primary=primary_id,repeat=ident,identical_wire=True,protocol_valid=outcome['protocol_valid'],verdict=outcome['verdict']))
# v54 Source recipes are unchanged from v53; prove every reused negative receipt.
import guarded_anthropic_repair as review,guarded_anthropic as api
controls=json.loads((P/'v53-negative-reuse.json').read_text())
for row in controls['rows']:
 saved=json.loads((E/(row['id']+'.request.json')).read_text());payload=json.loads(saved['payload']['messages'][0]['content']);packet=dict(case=next(x['case'] for x in g.packets()[1] if x['case']['source']==payload['original_source']),candidate=payload['candidate_batch'])
 expected=review.body(packet,'v54',saved['review_stage'],saved.get('review_effort','low'),saved.get('review_output_tokens'),saved['schema_revision']);g.require(expected==saved['payload'],'current negative recipe changed');api.verify_request_serialization(saved,expected,api.request_mode('v54'))
 checked=review.check(json.loads((E/(row['id']+'.judgment.json')).read_text()),packet,'v54',stage=saved['review_stage']);g.require(checked['verdict']=='reject','current negative false pass')
g.require(len(controls['rows'])==8,'eight negative receipts required')
# All ten same-recipe generation attempts, including failed prefixes, not just successes.
import guarded_anthropic_extraction as extraction
sequence=[('r1-ambiguous_ref','extraction-v49-schema-v3-output-6000'),('v23-fresh-003','extraction-v49-schema-v3-output-6000'),('v23-fresh-003','extraction-v50-schema-v3-output-6000-repair'),('r1-hypothetical','extraction-v49-schema-v3-output-6000'),('r1-hypothetical','extraction-v53-schema-v3-output-6000-repair'),('r1-nested_quote','extraction-v50-schema-v3-output-6000'),('r1-nested_quote','extraction-v50-schema-v3-output-6000-repair'),('r1-nested_quote','extraction-v53-schema-v3-output-6000-repair'),('v23-fresh-004','extraction-v50-schema-v3-output-6000'),('r1-habitual_temporal','extraction-v50-schema-v3-output-6000')]
for case,version in sequence:
 ident=f'focused-haiku-{version}-{case}-primary';saved=json.loads((E/(ident+'.request.json')).read_text());packet=next(p for p in g.packets()[1] if p['case']['id']==case)
 if saved.get('repair_context') is not None:packet=dict(packet,repair_context=saved['repair_context'])
 expected=extraction.body(packet,'extraction-v54',saved['output_format'],saved['extraction_output_tokens']);g.require(saved['payload']==expected,'generation sequence recipe mismatch');api.verify_request_serialization(saved,expected,api.request_mode('v54'))

g.require(len(rows)==6 and all(r['complete_reviews'] and r['revalidation']['experimental_gate_eligible'] and r['revalidation']['plan_hash_valid'] for r in rows),'not all six complete gates pass')
result=dict(start_commit='9511d09',implementation_commit='89c1d12',checkpoint_commit=subprocess.check_output(['git','rev-parse','HEAD']).decode().strip(),historical_request_rows_unchanged=len(old['requests']),ledger=dict(prior_nanos=budget[1],ceiling_nanos=budget[0],recorded_upper_nanos=recorded,cumulative_upper_nanos=budget[1]+recorded,remaining_nanos=budget[0]-budget[1]-recorded,requests=len(current),unknown_reservations=unknown,provider_billed_usd=None),six_exposed_sources=rows,negative_controls=negative,positive_repeat_checks=repeat_checks,current_negative_controls=dict(known_source_candidates=4,source_receipts=controls['rows'],deictic_negative_primary_and_repeat=reference[1:],failed_same_recipe_prefixes=boundary[:3]),required_verification=verification,limitations=['all current traces reconstruct v54; generator/Source receipt reuse requires exact wire equality','reused development Sources, not held-out evidence','no independent whole-corpus human gold','new-003,new-007,new-018,new-025,new-042 remain unassessed','all initial failures and exact-repeat disagreement retained'],production_approvals=0,completion_criteria_met=True,phase3_semantic_scope_accepted=True,acceptance_scope='six priority exposed Sources; bounded repairs; five negative candidates, no independent human gold or universal model-quality claim',generation_sequence=dict(primary_sources=6,first_pass_eligible=3,final_eligible=6,attempts_by_case={'r1-ambiguous_ref':1,'v23-fresh-003':2,'r1-hypothetical':2,'r1-nested_quote':3,'v23-fresh-004':1,'r1-habitual_temporal':1},same_recipe_extraction_attempts=10),production_default_promoted=False)
g.immutable(P/'current-audit-v54-final.json',result)
print(json.dumps(result,ensure_ascii=False,indent=2))
