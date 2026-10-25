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
version='extraction-v48-schema-v3-output-6000-repair'
def call(case,extra):
 cmd=[sys.executable,'evaluation/phase3/guarded_anthropic_repair.py','--case',case,'--version','v48','--candidate-version',version,'--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute']
 r=subprocess.run(cmd,capture_output=True,text=True);item=dict(command=cmd,exit_code=r.returncode,stdout=r.stdout,stderr=r.stderr);rows.append(item);(OUT/'v48-repairs-progress.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2));print(json.dumps(item),flush=True)
 if r.returncode:raise RuntimeError('no retry after dispatch failure')
 return json.loads(r.stdout)
try:
 for case in ['r1-ambiguous_ref','r1-nested_quote','v23-fresh-003']:
  passed=True
  candidate=json.loads((E/f'focused-haiku-{version}-{case}-primary.candidate.json').read_text())
  for i in range(len(candidate['claims'])):
   if call(case,['--reader-index',str(i)])['verdict']!='pass':passed=False;break
  if passed:
   for stage in review.stages('v48'):
    if call(case,['--stage',stage])['verdict']!='pass':break
  g.immutable(OUT/f'v48-repaired-{case}-trace.json',complete.replay(E,case,'extraction-v48',version))
finally:g.immutable(OUT/'v48-repairs-cohort.json',rows)
