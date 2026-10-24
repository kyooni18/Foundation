"""Frozen bounded semantic cohort. Never retry or select a later passing primary."""
import json,subprocess,sys
from pathlib import Path
ROOT=Path.cwd();OUT=ROOT/'reports/evidence/phase-3/six-dollar-continuation';E=ROOT/'reports/evidence/phase-3/semantic-repair';results=[]
def call(script,case,extra):
 cmd=[sys.executable,'evaluation/phase3/'+script,'--case',case,'--evidence',str(E),'--authorization',str(OUT/'authorization.json'),*extra,'--execute']
 p=subprocess.run(cmd,capture_output=True,text=True);row=dict(command=cmd,exit_code=p.returncode,stdout=p.stdout,stderr=p.stderr);results.append(row);print(json.dumps(row,ensure_ascii=False),flush=True)
 if p.returncode:raise RuntimeError('no retry')
 return json.loads(p.stdout)
def review(case,extra):return call('guarded_anthropic_repair.py',case,['--version','v37',*extra])
def source_stages(case,candidate):
 for stage in ['atomicity','fidelity','coverage']:
  r=review(case,['--candidate-version',candidate,'--stage',stage])
  if r.get('verdict')!='pass':break
try:
 for case in ['r1-hypothetical','v23-fresh-003','r1-nested_quote']:
  source_stages(case,'extraction-v35')
 for case in ['r1-nested_quote','v23-fresh-003','r1-ambiguous_ref']:
  review(case,[])
 for case in ['v23-fresh-003','r1-ambiguous_ref']:
  review(case,['--replicate','repeat'])
 # Current-profile generation is required before claiming current-profile quality;
 # these three source boundaries exercise narrator/time, unknown emotional target and Korean-dependent English.
 for case in ['r1-habitual_temporal','r1-ambiguous_ref','v23-fresh-004']:
  r=call('guarded_anthropic_extraction.py',case,['--version','extraction-v37'])
  if r.get('status')!='diagnostic_candidate_only':continue
  candidate=json.loads((E/f'focused-haiku-extraction-v37-{case}-primary.candidate.json').read_text())
  passed=True
  for i in range(len(candidate['claims'])):
   r=review(case,['--candidate-version','extraction-v37','--reader-index',str(i)])
   if r.get('verdict')!='pass':passed=False;break
  if passed:source_stages(case,'extraction-v37')
finally:
 with (OUT/'v37-cohort.json').open('x') as f:json.dump(results,f,ensure_ascii=False,indent=2)
