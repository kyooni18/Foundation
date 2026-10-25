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
items=[('v23-fresh-003','extraction-v49-schema-v3-output-6000',['--reader-index','0']),('r1-nested_quote','extraction-v50-schema-v3-output-6000',['--reader-index','0']),('r1-nested_quote','extraction-v50-schema-v3-output-6000-repair',['--reader-index','1']),('r1-nested_quote','extraction-v53-schema-v3-output-6000-repair',['--reader-index','1','--replicate','repeat']),('v23-fresh-003','extraction-v50-schema-v3-output-6000-repair',['--stage','fidelity','--replicate','repeat']),('r1-hypothetical','extraction-v53-schema-v3-output-6000-repair',['--stage','fidelity','--replicate','repeat'])]
g.immutable(OUT/'v54-final-boundary-selection.json',dict(maximum_calls=6,items=items,policy='Validate all three prior failed same-recipe prefixes and selected exact-request repeat stability. Any disagreement blocks semantic acceptance, no further retries or majority. Same cumulative cap.'))
try:
 for case,version,extra in items:
  cmd=[sys.executable,'evaluation/phase3/guarded_anthropic_repair.py','--case',case,'--version','v54','--candidate-version',version,'--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute'];p=subprocess.run(cmd,capture_output=True,text=True);row=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);rows.append(row);(OUT/'v54-final-boundary-progress.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2));print(json.dumps(row),flush=True)
  if p.returncode:raise RuntimeError('no retry after dispatch failure')
finally:g.immutable(OUT/'v54-final-boundary-results.json',rows)
