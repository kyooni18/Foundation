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
import guarded_anthropic as api
OUT=Path('reports/evidence/phase-3/resumed-haiku');E=Path('reports/evidence/phase-3/semantic-repair');rows=[]
CASES=[('v23-fresh-003','extraction-v50-schema-v3-output-6000-repair','v52'),('r1-hypothetical','extraction-v53-schema-v3-output-6000-repair','v53'),('r1-ambiguous_ref','extraction-v49-schema-v3-output-6000','v52'),('r1-nested_quote','extraction-v53-schema-v3-output-6000-repair','v54'),('v23-fresh-004','extraction-v50-schema-v3-output-6000','v52'),('r1-habitual_temporal','extraction-v50-schema-v3-output-6000','v52')]
probe=json.loads((OUT/'v54-reference-probe.json').read_text());g.require(len(probe)==3 and [json.loads(x['stdout'])['verdict'] for x in probe]==['pass','reject','reject'],'reference distinction probe failed')
g.immutable(OUT/'v54-cohort-selection.json',dict(cases=CASES,new_generations=0,new_readers_at_most=9,new_source_reviews_at_most=3,policy='All six current Sources, exact generator/Source-recipe reuse with new v54 readers. Only nested new Source reports needed. No invented review or majority. Previous reader false rejections retained.'))
def call(case,version,extra):
 cmd=[sys.executable,'evaluation/phase3/guarded_anthropic_repair.py','--case',case,'--version','v54','--candidate-version',version,'--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute']
 p=subprocess.run(cmd,capture_output=True,text=True);row=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);rows.append(row);(OUT/'v54-progress.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2));print(json.dumps(row),flush=True)
 if p.returncode:raise RuntimeError('no retry after failed dispatch')
 return json.loads(p.stdout)
try:
 for case,version,source_version in CASES:
  candidate=json.loads((E/f'focused-haiku-{version}-{case}-primary.candidate.json').read_text());passed=True
  for i,candidate_claim in enumerate(candidate['claims']):
   ident=f'focused-haiku-repair-v54-{case}-{version}-reader-{i}-primary'
   if (E/(ident+'.response.json')).exists():
    saved=json.loads((E/(ident+'.request.json')).read_text());expected=review.reader_body(candidate_claim['text'],'v54');g.require(saved['payload']==expected,'reader reuse recipe changed');api.verify_request_serialization(saved,expected,api.request_mode('v54'));result=review.reader_check(json.loads((E/(ident+'.judgment.json')).read_text()),'v54');rows.append(dict(reused_reader=ident,recipe_exact=True,verdict=result['verdict']))
   else:result=call(case,version,['--reader-index',str(i)])
   if result['verdict']!='pass':passed=False;break
  if passed and source_version=='v54':
   for stage in review.stages('v54'):
    if call(case,version,['--stage',stage])['verdict']!='pass':break
  t=complete.replay(E,case,'extraction-v54',version,source_versions={k:source_version for k in review.stages('v54')});g.immutable(OUT/f'v54-{case}-trace.json',t);print(json.dumps(dict(case=case,revalidation=t['revalidation'])),flush=True)
finally:g.immutable(OUT/'v54-cohort.json',rows)
