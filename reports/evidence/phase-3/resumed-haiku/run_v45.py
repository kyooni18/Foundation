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
CASES=['v23-fresh-003','v23-fresh-004','r1-ambiguous_ref','r1-nested_quote','r1-hypothetical','r1-habitual_temporal']
selection=[]
for case in CASES:
 version='extraction-v44-schema-v3' if case in CASES[:2] else 'extraction-v45-schema-v3'
 selection.append(dict(case=case,candidate_version=version,new_generation=case not in CASES[:2]))
g.immutable(OUT/'v45-selection.json',dict(cases=selection,maximum_new_extractions=4,maximum_new_readers=8,maximum_positive_source_reviews=54,maximum_negative_and_repeat_calls=10,policy='Review-only profile correction. Reuse the first two v44 generations and their actual readers only after full recipe and ordered-byte hash equality. Generate the other four once. Stop first nonpass per positive case; no retries. Negative comparisons retain four primary and four exact-request repeat trials. Two positive boundary repeats are diagnostic, never voting.'))
def call(script,case,extra):
 cmd=[sys.executable,'evaluation/phase3/'+script,'--case',case,'--version','extraction-v45' if 'extraction.py' in script else 'v45','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute']
 p=subprocess.run(cmd,capture_output=True,text=True);record=dict(kind='new_call',command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);rows.append(record);(OUT/'v45-progress.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2));print(json.dumps(record,ensure_ascii=False),flush=True)
 if p.returncode and any(x in p.stderr for x in ['provider transport failure','budget blocked','another paid dispatch']):raise RuntimeError('cohort stopped after dispatch failure; no retry')
 return json.loads(p.stdout) if p.returncode==0 else None
try:
 for selected in selection:
  case=selected['case'];candidate_version=selected['candidate_version'];reader_version='v45'
  packet=next(p for p in g.packets()[1] if p['case']['id']==case)
  if selected['new_generation']:
   result=call('guarded_anthropic_extraction.py',case,['--output-format','grounded-v3'])
   if result is None:continue
  else:
   ident=f'focused-haiku-{candidate_version}-{case}-primary'
   request=f.parse_judgment((E/(ident+'.request.json')).read_text());expected=extraction.body(packet,'extraction-v45','grounded-v3')
   g.require(request['payload']==expected,'reused extractor recipe changed');api.verify_request_serialization(request,expected,api.request_mode('extraction-v45'))
   rows.append(dict(kind='exact_byte_recipe_reuse',request_id=ident,request_sha256=f.sha((E/(ident+'.request.json')).read_bytes()),wire_sha256=request['wire_sha256']))
   reader_version='v44'
  path=E/f'focused-haiku-{candidate_version}-{case}-primary.candidate.json'
  if not path.exists():continue
  candidate=f.parse_judgment(path.read_text());passed=True
  for i,c in enumerate(candidate['claims']):
   if reader_version=='v44':
    ident=f'focused-haiku-repair-v44-{case}-{candidate_version}-reader-{i}-primary';request=f.parse_judgment((E/(ident+'.request.json')).read_text());expected=review.reader_body(c['text'],'v45');g.require(request['payload']==expected,'reader reuse changed');api.verify_request_serialization(request,expected,api.request_mode('v45'))
    response=f.parse_judgment((E/(ident+'.response.json')).read_text());g.require(response['stop_reason']=='end_turn','unchanged incomplete reader');result=review.reader_check(f.parse_judgment(''.join(b['text'] for b in response['content'] if b['type']=='text')));rows.append(dict(kind='exact_byte_recipe_reuse',request_id=ident,**result))
   else:result=call('guarded_anthropic_repair.py',case,['--candidate-version',candidate_version,'--reader-index',str(i)])
   if result is None or result.get('verdict')!='pass':passed=False;break
  if passed:
   for stage in review.stages('v45'):
    result=call('guarded_anthropic_repair.py',case,['--candidate-version',candidate_version,'--stage',stage])
    if result is None or result.get('verdict')!='pass':break
  try:g.immutable(OUT/f'v45-{case}-trace.json',complete.replay(E,case,'extraction-v45',candidate_version,reader_version))
  except (ValueError,KeyError,FileNotFoundError) as error:rows.append(dict(case=case,replay_error=str(error)))
 controls=[('r1-nested_quote',None,'fidelity'),('v23-fresh-003',None,'atomicity'),('r1-ambiguous_ref',None,'fidelity'),('v23-fresh-003','extraction-v38','fidelity')]
 for repeat in [False,True]:
  for case,candidate,stage in controls:call('guarded_anthropic_repair.py',case,['--stage',stage]+(['--candidate-version',candidate] if candidate else [])+(['--replicate','repeat'] if repeat else []))
 for case,stage in [('v23-fresh-003','fidelity'),('v23-fresh-004','korean')]:
  ident=f'focused-haiku-repair-v45-{case}-extraction-v44-schema-v3'+('' if stage=='fidelity' else '-'+stage)+'-schema-v3-primary.outcome.json'
  if (E/ident).exists() and f.parse_judgment((E/ident).read_text()).get('verdict')=='pass':call('guarded_anthropic_repair.py',case,['--candidate-version','extraction-v44-schema-v3','--stage',stage,'--replicate','repeat'])
finally:g.immutable(OUT/'v45-cohort.json',rows)
