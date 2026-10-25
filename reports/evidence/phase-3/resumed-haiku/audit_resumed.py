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
verification=json.loads((P/'verification-v50/results.json').read_text())
g.require(all(r['exit_code']==0 for r in verification),'required check failed')
selection=[('r1-ambiguous_ref','v50-r1-ambiguous_ref-trace.json'),('v23-fresh-003','v50-final-v23-fresh-003-trace.json'),('r1-hypothetical','v50-r1-hypothetical-trace.json'),('r1-nested_quote','v50-nested-repair-r1-nested_quote-trace.json'),('v23-fresh-004','v50-final-v23-fresh-004-trace.json'),('r1-habitual_temporal','v50-final-r1-habitual_temporal-trace.json')]
rows=[]
for case,name in selection:
 t=json.loads((P/name).read_text())
 rows.append(dict(case=case,trace=name,prompt=t['replay_row']['plan']['prompt_version'],claims=t['claim_count'],complete_reviews=t['complete_review_count'],revalidation=t['revalidation'],trace_sha256=f.sha((P/name).read_bytes()),independent_gold=False,editorial_withhold=case in {'r1-hypothetical'},production_approval=t['production_approval']))
negative={}
for version in ['v48','v49']:
 rs=json.loads((P/(version+'-negative-results.json')).read_text());negative[version]=dict(calls=len(rs),known_candidates=4,coherent_rejects=sum(r['exit_code']==0 and json.loads(r['stdout']).get('protocol_valid') is True and json.loads(r['stdout']).get('verdict')=='reject' for r in rs))
repeat_results=json.loads((P/'v50-repeat-results.json').read_text())
repeat_checks=[]
E=Path('reports/evidence/phase-3/semantic-repair')
for run in repeat_results:
 g.require(run['exit_code']==0,'repeat execution failed')
 outcome=json.loads(run['stdout']);ident=outcome['id']
 repeated=json.loads((E/(ident+'.request.json')).read_text())
 primary_id=ident.replace('-repeat-schema-v3-primary','-schema-v3-primary')
 if 'r1-ambiguous_ref' in ident:primary_id=primary_id.replace('repair-v50-','repair-v49-')
 primary=json.loads((E/(primary_id+'.request.json')).read_text())
 g.require(primary['payload']==repeated['payload'] and primary['wire_sha256']==repeated['wire_sha256'],'repeat request drift')
 repeat_checks.append(dict(primary=primary_id,repeat=ident,identical_wire=True,protocol_valid=outcome['protocol_valid'],verdict=outcome['verdict']))
result=dict(start_commit='9511d09',implementation_commit='852d4d2',checkpoint_commit=subprocess.check_output(['git','rev-parse','HEAD']).decode().strip(),historical_request_rows_unchanged=len(old['requests']),ledger=dict(prior_nanos=budget[1],ceiling_nanos=budget[0],recorded_upper_nanos=recorded,cumulative_upper_nanos=budget[1]+recorded,remaining_nanos=budget[0]-budget[1]-recorded,requests=len(current),unknown_reservations=unknown,provider_billed_usd=None),six_exposed_sources=rows,negative_controls=negative,positive_repeat_checks=repeat_checks,required_verification=verification,limitations=['all current traces reconstruct v50; generator/Source receipt reuse requires exact wire equality','reused development Sources, not held-out evidence','no independent whole-corpus human gold','new-003,new-007,new-018,new-025,new-042 remain unassessed','all initial failures and exact-repeat disagreement retained'],production_approvals=0,completion_tag=False,phase3_accepted=False,residual_failures=['conditional reader invents entity ownership and utterance binding despite structured reasons','delay reader and Source entailment disagree about the same unspecified affected-target argument'])
g.immutable(P/'current-audit.json',result)
print(json.dumps(result,ensure_ascii=False,indent=2))
