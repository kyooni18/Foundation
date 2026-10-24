"""Final bounded schema/coherence cohort, with exact unchanged-component reuse."""
import json,subprocess,sys
from pathlib import Path
ROOT=Path.cwd();OUT=ROOT/'reports/evidence/phase-3/six-dollar-continuation';E=ROOT/'reports/evidence/phase-3/semantic-repair';results=[]
def call(case,extra):
 cmd=[sys.executable,'evaluation/phase3/guarded_anthropic_repair.py','--case',case,'--version','v38','--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute']
 p=subprocess.run(cmd,capture_output=True,text=True);row=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);results.append(row);print(json.dumps(row,ensure_ascii=False),flush=True)
 if p.returncode:raise RuntimeError('no retries')
 return json.loads(p.stdout)
try:
 for i in range(2):
  r=call('v23-fresh-003',['--candidate-version','extraction-v38','--reader-index',str(i)])
  if r.get('verdict')!='pass':raise RuntimeError('repair reader failed; no Source review')
 for case,candidate in [('v23-fresh-004','extraction-v37'),('v23-fresh-003','extraction-v38'),('r1-hypothetical','extraction-v35'),('r1-nested_quote','extraction-v35'),('r1-habitual_temporal','extraction-v37'),('r1-ambiguous_ref','extraction-v37')]:
  for stage in ['atomicity','fidelity','coverage']:
   r=call(case,['--candidate-version',candidate,'--stage',stage])
   if r.get('verdict')!='pass':break
 for case in ['r1-nested_quote','v23-fresh-003','r1-ambiguous_ref']:
  call(case,[])
finally:
 with (OUT/'v38-cohort.json').open('x') as f:json.dump(results,f,ensure_ascii=False,indent=2)
