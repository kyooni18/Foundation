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
CASES=[('v23-fresh-003','extraction-v50-schema-v3-output-6000-repair'),('r1-hypothetical','extraction-v49-schema-v3-output-6000'),('r1-ambiguous_ref','extraction-v49-schema-v3-output-6000'),('r1-nested_quote','extraction-v50-schema-v3-output-6000-repair'),('v23-fresh-004','extraction-v50-schema-v3-output-6000'),('r1-habitual_temporal','extraction-v50-schema-v3-output-6000')]
selection=[]
for case,version in CASES:
 packet=next(p for p in g.packets()[1] if p['case']['id']==case);ident=f'focused-haiku-{version}-{case}-primary';request=f.parse_judgment((E/(ident+'.request.json')).read_text())
 if request.get('repair_context') is not None:packet=dict(packet,repair_context=request['repair_context'])
 expected=extraction.body(packet,'extraction-v52','grounded-v3',request.get('extraction_output_tokens'));g.require(request['payload']==expected,'extractor recipe changed');api.verify_request_serialization(request,expected,api.request_mode('extraction-v52'))
 selection.append(dict(case=case,candidate_version=version,exact_extractor_byte_recipe=True,request_sha256=f.sha((E/(ident+'.request.json')).read_bytes()),extraction_output_tokens=request['payload']['max_tokens'],bounded_repair=bool(request.get('repair_context'))))
negative=f.parse_judgment((OUT/'v52-negative-results.json').read_text())
reused=f.parse_judgment((OUT/'v52-negative-reuse.json').read_text())
g.require(len(reused['rows'])==6 and all(r['recipe_exactly_matches_v52'] and r['verdict']=='reject' for r in reused['rows']),'exact fidelity negative evidence required')
g.require(len(negative)==2 and all(r['exit_code']==0 and f.parse_judgment(r['stdout']).get('verdict')=='reject' for r in negative),'negative-control probe does not support this protocol')
g.immutable(OUT/'v52-cohort-selection.json',dict(cases=selection,new_generations=0,maximum_readers=10,maximum_positive_source_reviews=18,maximum_positive_repeat_calls=0,policy='All six priority Sources, selected current v50 cohort candidates and its actual repairs; exact v52 extraction recipe verified. Preserve every failed initial generation. Reuse generation only by complete ordered-byte recipe equality. New readers and Source reviews use the production-aligned generation order. Stop first nonpass, no retries.'))
def call(case,extra):
 cmd=[sys.executable,'evaluation/phase3/guarded_anthropic_repair.py','--case',case,'--version','v52','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute'];p=subprocess.run(cmd,capture_output=True,text=True);record=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);rows.append(record);(OUT/'v52-progress.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2));print(json.dumps(record,ensure_ascii=False),flush=True)
 if p.returncode and any(x in p.stderr for x in ['provider transport failure','budget blocked','another paid dispatch']):raise RuntimeError('dispatch failed; no retry')
 return json.loads(p.stdout) if p.returncode==0 else None
try:
 for case,version in CASES:
  candidate=f.parse_judgment((E/f'focused-haiku-{version}-{case}-primary.candidate.json').read_text());passed=True
  for i in range(len(candidate['claims'])):
   result=call(case,['--candidate-version',version,'--reader-index',str(i)])
   if result is None or result.get('verdict')!='pass':passed=False;break
  if passed:
   for stage in review.stages('v52'):
    result=call(case,['--candidate-version',version,'--stage',stage])
    if result is None or result.get('verdict')!='pass':break
  try:g.immutable(OUT/f'v52-{case}-trace.json',complete.replay(E,case,'extraction-v52',version))
  except (ValueError,KeyError,FileNotFoundError) as error:rows.append(dict(case=case,replay_error=str(error)))
 for case,stage in []:
  ident=f'focused-haiku-repair-v52-{case}-extraction-v44-schema-v3'+('' if stage=='fidelity' else '-'+stage)+'-schema-v3-primary.outcome.json'
  if (E/ident).exists() and f.parse_judgment((E/ident).read_text()).get('verdict')=='pass':call(case,['--candidate-version','extraction-v44-schema-v3','--stage',stage,'--replicate','repeat'])
finally:g.immutable(OUT/'v52-cohort.json',rows)
