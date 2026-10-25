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
OUT=Path('reports/evidence/phase-3/resumed-haiku');E=Path('reports/evidence/phase-3/semantic-repair');rows=[]
import guarded_anthropic_extraction as extraction,guarded_anthropic as api
CASES=[('v23-fresh-003','extraction-v44-schema-v3'),('v23-fresh-004','extraction-v44-schema-v3'),('r1-ambiguous_ref','extraction-v45-schema-v3-output-6000-repair'),('r1-nested_quote','extraction-v45-schema-v3-output-6000'),('r1-hypothetical','extraction-v45-schema-v3'),('r1-habitual_temporal','extraction-v45-schema-v3')]
selection=[]
for case,version in CASES:
 packet=next(p for p in g.packets()[1] if p['case']['id']==case);ident=f'focused-haiku-{version}-{case}-primary';request=f.parse_judgment((E/(ident+'.request.json')).read_text())
 if request.get('repair_context') is not None:packet=dict(packet,repair_context=request['repair_context'])
 expected=extraction.body(packet,'extraction-v46','grounded-v3',request.get('extraction_output_tokens'));g.require(request['payload']==expected,'extractor recipe changed');api.verify_request_serialization(request,expected,api.request_mode('extraction-v46'))
 selection.append(dict(case=case,candidate_version=version,exact_extractor_byte_recipe=True,request_sha256=f.sha((E/(ident+'.request.json')).read_bytes()),extraction_output_tokens=request['payload']['max_tokens'],bounded_repair=bool(request.get('repair_context'))))
g.immutable(OUT/'v46-selection.json',dict(cases=selection,new_generations=0,maximum_readers=10,maximum_positive_source_reviews=54,maximum_negative_and_repeat_calls=10,policy='All six priority Sources, exact frozen generated candidates, including the actual emotion repair and one higher-capacity nested generation. Preserve every failed initial generation. Reuse generation only by complete ordered-byte recipe equality. New readers and Source reviews use the production-aligned generation order. Stop first nonpass, no retries.'))
def call(case,extra):
 cmd=[sys.executable,'evaluation/phase3/guarded_anthropic_repair.py','--case',case,'--version','v46','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute'];p=subprocess.run(cmd,capture_output=True,text=True);record=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);rows.append(record);(OUT/'v46-progress.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2));print(json.dumps(record,ensure_ascii=False),flush=True)
 if p.returncode and any(x in p.stderr for x in ['provider transport failure','budget blocked','another paid dispatch']):raise RuntimeError('dispatch failed; no retry')
 return json.loads(p.stdout) if p.returncode==0 else None
try:
 for case,version in CASES:
  candidate=f.parse_judgment((E/f'focused-haiku-{version}-{case}-primary.candidate.json').read_text());passed=True
  for i in range(len(candidate['claims'])):
   result=call(case,['--candidate-version',version,'--reader-index',str(i)])
   if result is None or result.get('verdict')!='pass':passed=False;break
  if passed:
   for stage in review.stages('v46'):
    result=call(case,['--candidate-version',version,'--stage',stage])
    if result is None or result.get('verdict')!='pass':break
  try:g.immutable(OUT/f'v46-{case}-trace.json',complete.replay(E,case,'extraction-v46',version))
  except (ValueError,KeyError,FileNotFoundError) as error:rows.append(dict(case=case,replay_error=str(error)))
 controls=[('r1-nested_quote',None,'fidelity'),('v23-fresh-003',None,'atomicity'),('r1-ambiguous_ref',None,'fidelity'),('v23-fresh-003','extraction-v38','fidelity')]
 for repeat in [False,True]:
  for case,version,stage in controls:call(case,['--stage',stage]+(['--candidate-version',version] if version else [])+(['--replicate','repeat'] if repeat else []))
 for case,stage in [('v23-fresh-003','fidelity'),('v23-fresh-004','korean')]:
  ident=f'focused-haiku-repair-v46-{case}-extraction-v44-schema-v3'+('' if stage=='fidelity' else '-'+stage)+'-schema-v3-primary.outcome.json'
  if (E/ident).exists() and f.parse_judgment((E/ident).read_text()).get('verdict')=='pass':call(case,['--candidate-version','extraction-v44-schema-v3','--stage',stage,'--replicate','repeat'])
finally:g.immutable(OUT/'v46-cohort.json',rows)
