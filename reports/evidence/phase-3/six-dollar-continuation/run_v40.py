"""Bounded v40 cohort with exact-payload reuse, including failed unchanged reviews."""
import json,subprocess,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'evaluation/phase3'))
import guarded_anthropic_extraction as extraction
import guarded_anthropic_repair as review
import guarded_haiku as g
import focused_review as f
import replay_complete_semantic_trace as complete
ROOT=Path.cwd();OUT=ROOT/'reports/evidence/phase-3/six-dollar-continuation';E=ROOT/'reports/evidence/phase-3/semantic-repair';rows=[]
CASES=[('v23-fresh-003','extraction-v39'),('v23-fresh-004','extraction-v37'),('r1-ambiguous_ref','extraction-v37'),('r1-hypothetical','extraction-v35'),('r1-nested_quote','extraction-v35'),('r1-habitual_temporal','extraction-v37')]
def load(p):return f.parse_judgment(p.read_text())
def call(case,extra):
 cmd=[sys.executable,'evaluation/phase3/guarded_anthropic_repair.py','--case',case,'--version','v40','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute']
 p=subprocess.run(cmd,capture_output=True,text=True);record=dict(kind='paid',command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);rows.append(record)
 if p.returncode:print(json.dumps(record,ensure_ascii=False),flush=True);raise RuntimeError('no retry')
 result=json.loads(p.stdout);print(json.dumps(result,ensure_ascii=False),flush=True);return result

def reused(ident,packet,stage=None,index=None):
 request=load(E/(ident+'.request.json'))
 expected=review.reader_body(packet['candidate']['claims'][index]['text'],'v40') if index is not None else review.body(packet,'v40',stage)
 if request['payload']!=expected:return None
 response=load(E/(ident+'.response.json'))
 g.require(response['stop_reason']=='end_turn','unchanged incomplete review cannot be replaced')
 j=f.parse_judgment(''.join(b['text'] for b in response['content'] if b.get('type')=='text'))
 try:result=review.reader_check(j) if index is not None else review.check(j,packet,'v40',stage=stage)
 except (ValueError,KeyError,TypeError) as error:result=dict(protocol_valid=False,reason=str(error))
 rows.append(dict(kind='exact_request_reuse',request_id=ident,request_sha256=f.sha((E/(ident+'.request.json')).read_bytes()),**result))
 print(json.dumps(dict(reused=ident,**result),ensure_ascii=False),flush=True);return result

selection=[]
for case,version in CASES:
 packet=next(x for x in g.packets()[1] if x['case']['id']==case);request=load(E/f'focused-haiku-{version}-{case}-primary.request.json')
 if request.get('repair_context') is not None:packet=dict(packet,repair_context=request['repair_context'])
 g.require(request['payload']==extraction.body(packet,'extraction-v40'),'extractor recipe drift')
 selection.append(dict(case=case,candidate_version=version,exact_extractor_recipe_match=True,repair_context=bool(request.get('repair_context'))))
g.immutable(OUT/'v40-selection.json',dict(cases=selection,policy='Reuse only exact unchanged requests; never replace a failed unchanged primary. Atomicity, Source-meta and coverage questions changed. All other question recipes are byte-equal to v39. New v39 delay output is the one bounded re-extraction from actual attribution failure, not a text edit.'))
try:
 for case,version in CASES:
  packet=next(x for x in g.packets()[1] if x['case']['id']==case)
  packet=dict(packet,candidate=load(E/f'focused-haiku-{version}-{case}-primary.candidate.json'))
  prior=f'focused-haiku-repair-v39-{case}-{version}'
  reader_version='v39' if all((E/(prior+f'-reader-{i}-primary.request.json')).exists() for i in range(len(packet['candidate']['claims']))) else 'v40'
  passed=True
  for i in range(len(packet['candidate']['claims'])):
   result=reused(prior+f'-reader-{i}-primary',packet,index=i) if reader_version=='v39' else call(case,['--candidate-version',version,'--reader-index',str(i)])
   g.require(result is not None,'reader reuse recipe drift')
   if result.get('verdict')!='pass':passed=False;break
  sources={}
  if passed:
   for key,stage in review.stages('v40').items():
    ident=prior+('' if key=='fidelity' else '-'+key)+'-schema-v2-primary'
    result=reused(ident,packet,stage=stage) if (E/(ident+'.request.json')).exists() else None
    if result is not None:sources[key]='v39'
    else:result=call(case,['--candidate-version',version,'--stage',key]);sources[key]='v40'
    if result.get('verdict')!='pass':break
  g.immutable(OUT/f'v40-{case}-sources.json',sources)
  if passed:
   trace=complete.replay(E,case,'extraction-v40',version,reader_version,source_versions=sources)
   g.immutable(OUT/f'v40-{case}-trace.json',trace);print(json.dumps(dict(case=case,revalidation=trace['revalidation'])),flush=True)
 for case in ['r1-nested_quote','v23-fresh-003','r1-ambiguous_ref']:
  packet=next(x for x in g.packets()[1] if x['case']['id']==case)
  for key,stage in review.stages('v40').items():
   ident=f'focused-haiku-repair-v39-{case}'+('' if key=='fidelity' else '-'+key)+'-schema-v2-primary'
   result=reused(ident,packet,stage=stage) if (E/(ident+'.request.json')).exists() else None
   if result is None:result=call(case,['--stage',key])
   if result.get('verdict')!='pass':break
finally:
 with (OUT/'v40-cohort.json').open('x') as stream:json.dump(rows,stream,ensure_ascii=False,indent=2)
