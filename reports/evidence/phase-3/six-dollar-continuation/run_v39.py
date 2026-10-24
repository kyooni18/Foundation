"""Bounded responsibility-specific cohort; no retries or later-primary substitution."""
import json,subprocess,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'evaluation/phase3'))
import guarded_anthropic_extraction as extraction
import guarded_anthropic_repair as review
import guarded_haiku as g
import focused_review as f
ROOT=Path.cwd();OUT=ROOT/'reports/evidence/phase-3/six-dollar-continuation';E=ROOT/'reports/evidence/phase-3/semantic-repair';results=[]
CASES=[('v23-fresh-003','extraction-v35'),('v23-fresh-004','extraction-v37'),('r1-hypothetical','extraction-v35'),('r1-nested_quote','extraction-v35'),('r1-habitual_temporal','extraction-v37'),('r1-ambiguous_ref','extraction-v37')]
manifest=[]
for case,version in CASES:
 packet=next(x for x in g.packets()[1] if x['case']['id']==case)
 ident=f'focused-haiku-{version}-{case}-primary'
 request=f.parse_judgment((E/(ident+'.request.json')).read_text())
 g.require(request['payload']==extraction.body(packet,'extraction-v39'),'unchanged extractor recipe drift')
 manifest.append(dict(case=case,candidate_version=version,request_id=ident,request_sha256=f.sha((E/(ident+'.request.json')).read_bytes()),source_sha256=f.sha(packet['case']['source'].encode()),exact_extractor_recipe_match=True))
g.immutable(OUT/'v39-selection.json',dict(cases=manifest,source_review_stages=review.stages('v39'),selection='All six original priority Sources, initial generated candidate per frozen extraction recipe; later bad v38 repair retained as a separate negative control, never substituted for a passing initial candidate.',maximum_primary_reader_calls=10,maximum_primary_source_calls=54,negative_controls=['r1-nested_quote','v23-fresh-003','r1-ambiguous_ref']))
def call(case,extra):
 cmd=[sys.executable,'evaluation/phase3/guarded_anthropic_repair.py','--case',case,'--version','v39','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute']
 p=subprocess.run(cmd,capture_output=True,text=True);row=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);results.append(row);print(json.dumps(row,ensure_ascii=False),flush=True)
 if p.returncode:raise RuntimeError('execution failed; no automatic retry')
 return json.loads(p.stdout)
try:
 for case,version in CASES:
  candidate=f.parse_judgment((E/f'focused-haiku-{version}-{case}-primary.candidate.json').read_text())
  passed=True
  for i in range(len(candidate['claims'])):
   r=call(case,['--candidate-version',version,'--reader-index',str(i)])
   if r.get('verdict')!='pass':passed=False;break
  if not passed:continue
  for stage in review.stages('v39'):
   r=call(case,['--candidate-version',version,'--stage',stage])
   if r.get('verdict')!='pass':break
 for case in ['r1-nested_quote','v23-fresh-003','r1-ambiguous_ref']:
  for stage in review.stages('v39'):
   r=call(case,['--stage',stage])
   if r.get('verdict')!='pass':break
finally:
 with (OUT/'v39-cohort.json').open('x') as stream:json.dump(results,stream,ensure_ascii=False,indent=2)
