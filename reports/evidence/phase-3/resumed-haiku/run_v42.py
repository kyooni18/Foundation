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
CASES=['v23-fresh-003','v23-fresh-004','r1-ambiguous_ref','r1-nested_quote','r1-hypothetical','r1-habitual_temporal']
g.immutable(OUT/'v42-selection.json',dict(cases=[dict(case=c,source_sha256=f.sha(next(p for p in g.packets()[1] if p['case']['id']==c)['case']['source'].encode())) for c in CASES],maximum_extractions=6,maximum_readers=12,maximum_source_reviews=54,maximum_negative_source_reviews=36,policy='Single primary per new generator+wire recipe; stop first failed gate per case and stop cohort on transport failure. No automatic retries, no passing substitution. Negative controls remain actual archived outputs. All unknown charges remain held; one exclusive paid dispatch.',key_source='interactive zsh configuration; key excluded'))
def call(script,case,extra):
 cmd=[sys.executable,'evaluation/phase3/'+script,'--case',case,'--version','extraction-v42' if 'extraction.py' in script else 'v42','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute']
 p=subprocess.run(cmd,capture_output=True,text=True);record=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);rows.append(record);(OUT/'v42-progress.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2));print(json.dumps(record,ensure_ascii=False),flush=True)
 if p.returncode and any(x in p.stderr for x in ['provider transport failure','budget blocked','another paid dispatch']):raise RuntimeError('cohort stopped after dispatch failure; no automatic retry')
 return json.loads(p.stdout) if p.returncode==0 else None
try:
 for case in CASES:
  result=call('guarded_anthropic_extraction.py',case,['--output-format','contract-v1'])
  path=E/f'focused-haiku-extraction-v42-schema-v1-{case}-primary.candidate.json'
  if result is None or not path.exists():continue
  candidate=f.parse_judgment(path.read_text());passed=True
  for i in range(len(candidate['claims'])):
   result=call('guarded_anthropic_repair.py',case,['--candidate-version','extraction-v42-schema-v1','--reader-index',str(i)])
   if result is None or result.get('verdict')!='pass':passed=False;break
  if passed:
   for stage in review.stages('v42'):
    result=call('guarded_anthropic_repair.py',case,['--candidate-version','extraction-v42-schema-v1','--stage',stage])
    if result is None or result.get('verdict')!='pass':break
  try:g.immutable(OUT/f'v42-{case}-trace.json',complete.replay(E,case,'extraction-v42','extraction-v42-schema-v1'))
  except (ValueError,KeyError,FileNotFoundError) as error:rows.append(dict(case=case,replay_error=str(error)))
 for case,candidate in [('r1-nested_quote',None),('v23-fresh-003',None),('r1-ambiguous_ref',None),('v23-fresh-003','extraction-v38')]:
  for stage in review.stages('v42'):
   result=call('guarded_anthropic_repair.py',case,['--stage',stage]+(['--candidate-version',candidate] if candidate else []))
   if result is None or result.get('verdict')!='pass':break
finally:g.immutable(OUT/'v42-cohort.json',rows)
