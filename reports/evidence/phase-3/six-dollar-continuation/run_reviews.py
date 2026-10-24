"""Bounded review experiment; existing extraction/reader recipes stay unchanged."""
import json,subprocess,sys
from pathlib import Path
ROOT=Path.cwd();OUT=ROOT/'reports/evidence/phase-3/six-dollar-continuation';E=ROOT/'reports/evidence/phase-3/semantic-repair'
results=[]
def call(case,extra):
 cmd=[sys.executable,'evaluation/phase3/guarded_anthropic_repair.py','--case',case,'--version','v36','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute']
 p=subprocess.run(cmd,capture_output=True,text=True);row=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);results.append(row);print(json.dumps(row,ensure_ascii=False),flush=True)
 if p.returncode:raise RuntimeError('no retries')
 return json.loads(p.stdout)
try:
 for case,stages in [('r1-hypothetical',['fidelity','coverage']),('v23-fresh-003',['atomicity','fidelity','coverage']),('r1-nested_quote',['atomicity','fidelity','coverage'])]:
  for stage in stages:
   r=call(case,['--candidate-version','extraction-v35','--stage',stage])
   if r.get('verdict')!='pass':break
 for case in ['r1-nested_quote','v23-fresh-003','r1-ambiguous_ref']:
  call(case,[])
 call('r1-nested_quote',['--replicate','repeat'])
finally:
 with (OUT/'review-cohort.json').open('x') as f:json.dump(results,f,ensure_ascii=False,indent=2)
