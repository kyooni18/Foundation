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
g.immutable(OUT/'v45-capacity-selection.json',dict(maximum_extractions=2,cases=[dict(case='r1-nested_quote',basis='Actual primary used all 2500 tokens in thinking and produced no text; one explicit 6000-token capacity variant.'),dict(case='r1-ambiguous_ref',basis='One bounded repair from actual failed v45 trace; no passing-primary substitution.')],output_tokens=6000,policy='No further generation attempt after these two; all initial failures remain recorded.'))
try:
 for case,trace in [('r1-nested_quote',None),('r1-ambiguous_ref','v45-r1-ambiguous_ref-trace.json')]:
  cmd=[sys.executable,'evaluation/phase3/guarded_anthropic_extraction.py','--case',case,'--version','extraction-v45','--output-format','grounded-v3','--output-tokens','6000','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),'--execute']
  if trace:cmd+=['--replicate','repair','--repair-from-trace',str(OUT/trace)]
  p=subprocess.run(cmd,capture_output=True,text=True);row=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);rows.append(row);print(json.dumps(row,ensure_ascii=False),flush=True)
  if p.returncode:break
finally:g.immutable(OUT/'v45-capacity-results.json',rows)
