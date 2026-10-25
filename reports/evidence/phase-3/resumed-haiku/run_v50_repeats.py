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
for case,version in [('r1-ambiguous_ref','extraction-v49-schema-v3-output-6000'),('v23-fresh-004','extraction-v50-schema-v3-output-6000')]:
 cmd=[sys.executable,'evaluation/phase3/guarded_anthropic_repair.py','--case',case,'--version','v50','--candidate-version',version,'--stage','fidelity','--replicate','repeat','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),'--execute']
 r=subprocess.run(cmd,capture_output=True,text=True);item=dict(command=cmd,exit_code=r.returncode,stdout=r.stdout,stderr=r.stderr);rows.append(item);print(json.dumps(item),flush=True)
 if r.returncode:break
g.immutable(OUT/'v50-repeat-results.json',rows)
