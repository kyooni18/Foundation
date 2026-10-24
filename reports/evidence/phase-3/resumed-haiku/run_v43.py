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
CASES=['v23-fresh-003','v23-fresh-004']
g.immutable(OUT/'v43-selection.json',dict(cases=CASES,maximum_extractions=2,maximum_readers=4,maximum_positive_source_reviews=18,negative_controls=dict(nested='fidelity',fused='atomicity',causal='fidelity',grammar='fidelity'),maximum_negative_calls=6,policy='New evidence-first recipe first tests the two demonstrated extraction boundaries. One primary each; failed outputs remain failures. Negative controls use their relevant responsibility directly. No retries. Two preselected negative repeats compare exact requests.'))
def call(script,case,extra):
 cmd=[sys.executable,'evaluation/phase3/'+script,'--case',case,'--version','extraction-v43' if 'extraction.py' in script else 'v43','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute']
 p=subprocess.run(cmd,capture_output=True,text=True);record=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);rows.append(record);(OUT/'v43-progress.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2));print(json.dumps(record,ensure_ascii=False),flush=True)
 if p.returncode and any(x in p.stderr for x in ['provider transport failure','budget blocked','another paid dispatch']):raise RuntimeError('cohort stopped after dispatch failure; no automatic retry')
 return json.loads(p.stdout) if p.returncode==0 else None
try:
 for case in CASES:
  result=call('guarded_anthropic_extraction.py',case,['--output-format','grounded-v2'])
  path=E/f'focused-haiku-extraction-v43-schema-v2-{case}-primary.candidate.json'
  if result is None or not path.exists():continue
  candidate=f.parse_judgment(path.read_text());passed=True
  for i in range(len(candidate['claims'])):
   result=call('guarded_anthropic_repair.py',case,['--candidate-version','extraction-v43-schema-v2','--reader-index',str(i)])
   if result is None or result.get('verdict')!='pass':passed=False;break
  if passed:
   for stage in review.stages('v43'):
    result=call('guarded_anthropic_repair.py',case,['--candidate-version','extraction-v43-schema-v2','--stage',stage])
    if result is None or result.get('verdict')!='pass':break
  try:g.immutable(OUT/f'v43-{case}-trace.json',complete.replay(E,case,'extraction-v43','extraction-v43-schema-v2'))
  except (ValueError,KeyError,FileNotFoundError) as error:rows.append(dict(case=case,replay_error=str(error)))
 for case,candidate,stage,repeat in [('r1-nested_quote',None,'fidelity',False),('v23-fresh-003',None,'atomicity',False),('r1-ambiguous_ref',None,'fidelity',False),('v23-fresh-003','extraction-v38','fidelity',False),('r1-nested_quote',None,'fidelity',True),('v23-fresh-003','extraction-v38','fidelity',True)]:
  call('guarded_anthropic_repair.py',case,['--stage',stage]+(['--candidate-version',candidate] if candidate else [])+(['--replicate','repeat'] if repeat else []))
finally:g.immutable(OUT/'v43-cohort.json',rows)
