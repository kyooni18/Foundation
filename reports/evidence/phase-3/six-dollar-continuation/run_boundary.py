"""One bounded v35 cohort: three previously problematic frozen Sources; no retries."""
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path.cwd()
EVIDENCE=ROOT/'reports/evidence/phase-3/semantic-repair'
OUT=ROOT/'reports/evidence/phase-3/six-dollar-continuation'
AUTH=OUT/'authorization.json'
CASES=['r1-hypothetical','v23-fresh-003','r1-nested_quote']
results=[]

def call(script,case,*extra):
    cmd=[sys.executable,str(ROOT/'evaluation/phase3'/script),'--case',case,
        '--evidence',str(EVIDENCE),'--authorization',str(AUTH),*extra,'--execute']
    done=subprocess.run(cmd,capture_output=True,text=True)
    row=dict(case=case,command=cmd,exit_code=done.returncode,stdout=done.stdout,stderr=done.stderr)
    results.append(row)
    print(json.dumps(row,ensure_ascii=False),flush=True)
    if done.returncode: raise RuntimeError('execution failed; no automatic retry')
    return json.loads(done.stdout)

try:
    for case in CASES:
        generated=call('guarded_anthropic_extraction.py',case,'--version','extraction-v35')
        if generated.get('status')!='diagnostic_candidate_only':continue
        path=EVIDENCE/f'focused-haiku-extraction-v35-{case}-primary.candidate.json'
        candidate=json.loads(path.read_text())
        passed=True
        for i in range(len(candidate['claims'])):
            checked=call('guarded_anthropic_repair.py',case,'--version','v35',
                '--candidate-version','extraction-v35','--reader-index',str(i))
            if checked.get('verdict')!='pass':passed=False;break
        if not passed:continue
        for stage in ['atomicity','fidelity','coverage']:
            checked=call('guarded_anthropic_repair.py',case,'--version','v35',
                '--candidate-version','extraction-v35','--stage',stage)
            if checked.get('verdict')!='pass':break
finally:
    target=OUT/'boundary-cohort.json'
    with target.open('x') as stream:json.dump(dict(cases=CASES,requests=results),stream,ensure_ascii=False,indent=2)
