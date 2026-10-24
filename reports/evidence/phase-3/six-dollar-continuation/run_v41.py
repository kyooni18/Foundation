"""Single bounded v41 cohort, preserving every failed primary and stopping its prefix."""
import json,subprocess,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'evaluation/phase3'))
import guarded_haiku as g
import focused_review as f
import guarded_anthropic_repair as review
import replay_complete_semantic_trace as complete
OUT=Path('reports/evidence/phase-3/six-dollar-continuation');E=Path('reports/evidence/phase-3/semantic-repair');rows=[]
CASES=['v23-fresh-003','v23-fresh-004','r1-ambiguous_ref','r1-nested_quote','r1-hypothetical','r1-habitual_temporal']
g.immutable(OUT/'v41-selection.json',dict(cases=[dict(case=c,source_sha256=f.sha(next(p for p in g.packets()[1] if p['case']['id']==c)['case']['source'].encode())) for c in CASES],maximum_extractions=6,maximum_reader_calls=12,maximum_positive_source_calls=54,negative_controls=['old nested gloss','old fused delay','old causal emotion','actual v38 grammatical commentary'],policy='One new primary per changed generator recipe; stop at first nonpass; no retries, passing substitution or text editing.'))
def call(script,case,extra):
 cmd=[sys.executable,'evaluation/phase3/'+script,'--case',case,'--version','extraction-v41' if 'extraction.py' in script else 'v41','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute']
 p=subprocess.run(cmd,capture_output=True,text=True);row=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);rows.append(row);(OUT/'v41-progress.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2));print(json.dumps(row,ensure_ascii=False),flush=True)
 if p.returncode: return None
 return json.loads(p.stdout)
try:
 for case in CASES:
  r=call('guarded_anthropic_extraction.py',case,[])
  if r is None:continue
  path=E/f'focused-haiku-extraction-v41-{case}-primary.candidate.json'
  if not path.exists():continue
  candidate=f.parse_judgment(path.read_text());passed=True
  for i in range(len(candidate['claims'])):
   r=call('guarded_anthropic_repair.py',case,['--candidate-version','extraction-v41','--reader-index',str(i)])
   if r is None or r.get('verdict')!='pass':passed=False;break
  if passed:
   for key in review.stages('v41'):
    r=call('guarded_anthropic_repair.py',case,['--candidate-version','extraction-v41','--stage',key])
    if r is None or r.get('verdict')!='pass':break
  try:g.immutable(OUT/f'v41-{case}-trace.json',complete.replay(E,case,'extraction-v41'))
  except (ValueError,FileNotFoundError,KeyError) as error:rows.append(dict(case=case,replay_error=str(error)))
 for case,candidate in [('r1-nested_quote',None),('v23-fresh-003',None),('r1-ambiguous_ref',None),('v23-fresh-003','extraction-v38')]:
  for key in review.stages('v41'):
   extra=['--stage',key]+(['--candidate-version',candidate] if candidate else [])
   r=call('guarded_anthropic_repair.py',case,extra)
   if r is None or r.get('verdict')!='pass':break
finally:g.immutable(OUT/'v41-cohort.json',rows)
