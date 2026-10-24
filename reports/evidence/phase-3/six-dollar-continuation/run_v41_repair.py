"""One trace-linked repair per demonstrated failed v41 boundary, not a new primary."""
import json,subprocess,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'evaluation/phase3'))
import guarded_haiku as g, focused_review as f, guarded_anthropic_repair as review, replay_complete_semantic_trace as complete
OUT=Path('reports/evidence/phase-3/six-dollar-continuation');E=Path('reports/evidence/phase-3/semantic-repair');rows=[]
cases=[('v23-fresh-003','v41-v23-fresh-003-trace.json'),('v23-fresh-004','v41-v23-fresh-004-structural-trace.json'),('r1-ambiguous_ref','v41-r1-ambiguous_ref-trace.json')]
g.immutable(OUT/'v41-repair-selection.json',dict(cases=cases,maximum_new_extractions=3,maximum_readers=6,maximum_source_reviews=27,policy='Exactly one bounded repair using actual failed trace and unchanged v41 prompts; primary failures remain denominator; stop first failed review, no retry.'))
def call(script,case,extra):
 cmd=[sys.executable,'evaluation/phase3/'+script,'--case',case,'--version','extraction-v41' if 'extraction.py' in script else 'v41','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute']
 p=subprocess.run(cmd,capture_output=True,text=True);row=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);rows.append(row);(OUT/'v41-repair-progress.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2));print(json.dumps(row,ensure_ascii=False),flush=True)
 return json.loads(p.stdout) if p.returncode==0 else None
try:
 for case,trace in cases:
  r=call('guarded_anthropic_extraction.py',case,['--replicate','repair','--repair-from-trace',str(OUT/trace)])
  if r is None:continue
  path=E/f'focused-haiku-extraction-v41-repair-{case}-primary.candidate.json'
  if not path.exists():continue
  candidate=f.parse_judgment(path.read_text());passed=True
  for i in range(len(candidate['claims'])):
   r=call('guarded_anthropic_repair.py',case,['--candidate-version','extraction-v41-repair','--reader-index',str(i)])
   if r is None or r.get('verdict')!='pass':passed=False;break
  if passed:
   for key in review.stages('v41'):
    r=call('guarded_anthropic_repair.py',case,['--candidate-version','extraction-v41-repair','--stage',key])
    if r is None or r.get('verdict')!='pass':break
  try:g.immutable(OUT/f'v41-repair-{case}-trace.json',complete.replay(E,case,'extraction-v41','extraction-v41-repair'))
  except (ValueError,FileNotFoundError,KeyError) as error:rows.append(dict(case=case,replay_error=str(error)))
finally:g.immutable(OUT/'v41-repair-cohort.json',rows)
