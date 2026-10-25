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
controls=[('r1-nested_quote',None,'fidelity'),('v23-fresh-003',None,'atomicity'),('r1-ambiguous_ref',None,'fidelity'),('v23-fresh-003','extraction-v38','fidelity')]
g.immutable(OUT/'v51-negative-selection.json',dict(controls=controls,maximum_calls=8,policy='Probe all four known bad boundaries and one exact-request repeat each before positive evaluation. Any non-reject is retained and blocks promotion of this experimental protocol; no majority or substituted primary.',all_ten_dimensions_retained=True,source_questions=3))
try:
 for repeat in [False,True]:
  for case,version,stage in controls:
   cmd=[sys.executable,'evaluation/phase3/guarded_anthropic_repair.py','--case',case,'--version','v51','--stage',stage,'--evidence',str(E),'--authorization',str(OUT/'authorization.json'),'--execute']+(['--candidate-version',version] if version else [])+(['--replicate','repeat'] if repeat else [])
   p=subprocess.run(cmd,capture_output=True,text=True);row=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);rows.append(row);(OUT/'v51-negative-progress.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2));print(json.dumps(row,ensure_ascii=False),flush=True)
   if p.returncode:raise RuntimeError('no retry after failed dispatch')
finally:g.immutable(OUT/'v51-negative-results.json',rows)
