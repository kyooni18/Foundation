"""One bounded structured current-profile cohort, using the user's interactive-shell key."""
import json,os,subprocess,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'evaluation/phase3'))
import guarded_haiku as g,focused_review as f,guarded_anthropic_repair as review,replay_complete_semantic_trace as complete
# Read only the configured key through a private pipe; discard all shell startup output.
rfd,wfd=os.pipe()
try:
 code=f'import os;os.write({wfd},os.environ.get("ANTHROPIC_API_KEY","").encode())'
 import shlex
 proc=subprocess.run(['zsh','-ic','python3 -c '+shlex.quote(code)],pass_fds=(wfd,),stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 os.close(wfd);wfd=-1
 secret=os.read(rfd,1025).decode();g.require(proc.returncode==0 and 0<len(secret)<1024,'configured shell key unavailable')
 os.environ['ANTHROPIC_API_KEY']=secret
finally:
 os.close(rfd)
 if wfd>=0:os.close(wfd)
OUT=Path('reports/evidence/phase-3/resumed-haiku/thinking-probe');OUT.mkdir(exist_ok=True);E=Path('reports/evidence/phase-3/semantic-repair');rows=[]
from source_first_experiment import call
import guarded_anthropic as api
AUTH=f.parse_judgment(Path('reports/evidence/phase-3/resumed-haiku/authorization.json').read_text())
trials=[('bad-nested','r1-nested_quote',None,'fidelity'),('bad-fused','v23-fresh-003',None,'atomicity'),('bad-causal','r1-ambiguous_ref',None,'fidelity'),('bad-grammar','v23-fresh-003','extraction-v38','fidelity'),('good-delay','v23-fresh-003','extraction-v44-schema-v3','fidelity'),('good-mixed','v23-fresh-004','extraction-v44-schema-v3','fidelity')]
g.immutable(OUT/'selection.json',dict(trials=trials,maximum_calls=6,changes=['dimension reason property before status','thinking disabled','effort high'],semantic_policy='v47 three questions, all ten checks; no verdict normalization',comparison='Joint diagnostic; cannot isolate one parameter effect.',official_source='https://platform.claude.com/docs/en/build-with-claude/thinking-troubleshooting',independent_ground_truth=False))
try:
 for label,case,version,key in trials:
  packet=next(p for p in g.packets()[1] if p['case']['id']==case)
  if version:packet=dict(packet,candidate=f.parse_judgment((E/f'focused-haiku-{version}-{case}-primary.candidate.json').read_text()))
  stage=review.stages('v47')[key];body=review.body(packet,'v47',stage)
  for dimension in body['output_config']['format']['schema']['properties']['claims']['items']['properties']['dimensions']['properties'].values():dimension['properties']={k:dimension['properties'][k] for k in ['reason','status']}
  body['thinking']=dict(type='disabled');body['output_config']['effort']='high'
  ident='focused-haiku-v47-disabled-high-'+label+'-primary'
  judgment=call(ident,body,OUT,AUTH,'ordered-json-v2')
  try:result=review.check(judgment,packet,'v47',stage=stage)
  except (ValueError,KeyError,TypeError) as error:result=dict(protocol_valid=False,reason=str(error),model_verdict=judgment.get('verdict'))
  g.immutable(OUT/(ident+'.judgment.json'),judgment);g.immutable(OUT/(ident+'.outcome.json'),result);row=dict(id=ident,case=case,kind=label,**result);rows.append(row);print(__import__('json').dumps(row,ensure_ascii=False),flush=True)
finally:g.immutable(OUT/'comparison.json',dict(rows=rows,paid_calls=len(rows),production_approval=False,independent_semantic_ground_truth=False))
