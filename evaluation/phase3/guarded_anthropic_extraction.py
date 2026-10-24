"""One bounded direct-Claude extraction call on a frozen focused Source.

Diagnostic only. Reuses the existing direct Anthropic transport and SQLite budget;
never retries, approves, or publishes generated Claims.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

import focused_review as focused
import guarded_anthropic as api
import guarded_haiku as budget

VARIANT='audited'
ALLOWED={'r1-nested_quote','r1-ambiguous_ref','v23-fresh-003',
         'r1-hypothetical','r1-habitual_temporal','v23-fresh-004'}


def rust_structure(candidate,case):
    """Authoritative offline preflight; no model, approval or gate credit."""
    row=dict(corpus_case=case,source_hash=focused.sha(case['source'].encode()),
             raw=focused.canonical(candidate).decode(),variant=VARIANT,plan=None,reviews=[])
    with tempfile.TemporaryDirectory(prefix='prime-radiant-preflight-') as temporary:
        src=Path(temporary)/'input.jsonl';dst=Path(temporary)/'output.jsonl'
        src.write_bytes(focused.canonical(row)+b'\n')
        result=subprocess.run(['cargo','run','--quiet','--offline','--example','revalidate_extraction',
            '--','--input',str(src),'--output',str(dst)],cwd=focused.ROOT,capture_output=True,text=True)
        budget.require(result.returncode==0,'Rust preflight failed: '+result.stderr)
        return focused.parse_judgment(dst.read_text())['revalidation']

def prompt(version):
    root=focused.ROOT/'prompts'
    contract=(root/'extraction-v12/contract.txt').read_text()
    frames=(root/version/'frames.txt').read_text()
    variant=json.loads((root/'extraction-v1/variants.json').read_text())[VARIANT]
    return f'{contract}\n{frames}\n{variant}'

def body(packet,version):
    source=packet['case']
    content=dict(original_source=source['source'],authored_at=source.get('authored_at'),
        observed_at=None,artifact_available=False,previous_failure_code=None,
        prior_extraction_claims=[],
        repair_instruction='Re-analyze the original source and repair the entire batch; do not omit difficult propositions to avoid failure.')
    system=prompt(version)
    user=focused.canonical(content).decode()
    budget.require(len(system.encode())+len(user.encode())<=256*1024,'oversized context')
    return dict(model=api.MODEL,max_tokens=(2500 if version in {'extraction-v29','extraction-v30','extraction-v31','extraction-v32','extraction-v33','extraction-v34','extraction-v35','extraction-v36'} else 6000),system=system,
        messages=[dict(role='user',content=user)],
        output_config=dict(effort='low'),service_tier='standard_only')

def structural(raw,source):
    result=focused.parse_judgment(raw)
    budget.require(set(result)=={'claims','empty_reason'} and
        isinstance(result['claims'],list),'invalid batch envelope')
    budget.require(bool(result['claims']) != bool(result['empty_reason']),
                   'empty batch mismatch')
    for claim in result['claims']:
        budget.require(set(claim)=={'text','lang','category','speaker','speaker_label',
           'basis','anchor','tkind','applies_from','applies_to','standing',
           'index_fields','revision_of','mentions','dependencies'},'invalid Claim fields')
        anchor=claim['anchor']
        budget.require(set(anchor)=={'quote','occurrence'} and
                       type(anchor['occurrence']) is int and anchor['occurrence']>=0,
                       'invalid evidence anchor')
        quote=anchor['quote']
        positions=[]
        if quote:
            start=source.find(quote)
            while start>=0:
                positions.append(start)
                start=source.find(quote,start+1)
        budget.require(anchor['occurrence']<len(positions),'nonexact source anchor')
    return result

def dispatch(packet,evidence,authorization,version):
    case=packet['case']['id']
    ident=f'focused-haiku-{version}-{case}-primary'
    key=os.environ.get('ANTHROPIC_API_KEY')
    budget.require(key and len(key)<1024 and '\n' not in key and '\r' not in key,
                   'ANTHROPIC_API_KEY not configured')
    d=api.Dispatcher(budget.LEDGER,evidence,api.transport(key),authorization)
    conn=d.connection()
    try:
        d.check(conn,1)
        budget.require(conn.execute('SELECT 1 FROM request WHERE id=?',(ident,)).fetchone() is None,
                       'already attempted; no retry')
        request=body(packet,version)
        quote=api.quote(request)
        d.check(conn,quote['estimate'])
        model=d.transport('GET','/v1/models/'+api.MODEL,None)
        budget.require(model.get('id')==api.MODEL,'unexpected model identity')
        conn.execute('BEGIN IMMEDIATE')
        _,cumulative=d.check(conn,quote['estimate'])
        conn.execute('INSERT INTO request(id,model,created_at,estimate,cumulative) '
                     "VALUES(?,?,datetime('now'),?,?)",
                     (ident,'anthropic/'+api.MODEL,quote['estimate'],cumulative))
        budget.immutable(evidence/(ident+'.request.json'),dict(id=ident,payload=request,
            source_sha256=focused.sha(packet['case']['source'].encode()),
            frozen_packet_sha256=focused.sha(focused.canonical(packet)),
            prompt_sha256=focused.sha(request['system'].encode()),quote=quote,
            model_info=model,authorization=authorization))
        conn.commit()
        response=d.transport('POST','/v1/messages',request)
        budget.immutable(evidence/(ident+'.response.json'),response)
        usage=api.conservative_usage(response)
        budget.require(isinstance(response.get('id'),str) and 0<len(response['id'])<=256,
                       'invalid response ID')
        conn.execute('BEGIN IMMEDIATE')
        changed=conn.execute("UPDATE request SET actual=?,provider_id=?,settled_at=datetime('now'),"
            'prompt_tokens=?,completion_tokens=? WHERE id=? AND actual IS NULL',
            (usage['conservative_upper_nanos'],'anthropic:upper:'+response['id'],
             usage['input_tokens'],usage['output_tokens'],ident)).rowcount
        budget.require(changed==1,'already settled')
        conn.execute('UPDATE request SET cumulative=(SELECT prior FROM budget)+(SELECT '
                     'SUM(COALESCE(actual,estimate)) FROM request) WHERE id=?',(ident,))
        conn.commit()
        budget.immutable(evidence/(ident+'.accounting.json'),usage)
        budget.require(usage['conservative_upper_nanos']<=quote['estimate'],'usage bound exceeds reservation; stop')
        budget.require(response['stop_reason']=='end_turn','incomplete model response')
        raw=''.join(b['text'] for b in response['content'] if b.get('type')=='text')
        try:
            candidate=structural(raw,packet['case']['source'])
        except (ValueError,KeyError,TypeError) as error:
            budget.immutable(evidence/(ident+'.rejection.json'),dict(status='structural_invalid',
                reason=str(error),semantic_acceptance=None,production_approval=False))
            return dict(id=ident,status='structural_invalid',reason=str(error))
        budget.immutable(evidence/(ident+'.candidate.json'),candidate)
        result=dict(status='diagnostic_candidate_only',claims=len(candidate['claims']),
            semantic_acceptance=None,production_approval=False)
        budget.immutable(evidence/(ident+'.outcome.json'),result)
        return dict(id=ident,**result)
    finally:
        conn.close()

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--case',required=True)
    parser.add_argument('--evidence',type=Path,required=True)
    parser.add_argument('--authorization',type=Path,required=True)
    parser.add_argument('--version',choices=['extraction-v25','extraction-v26','extraction-v28','extraction-v29','extraction-v30','extraction-v31','extraction-v32','extraction-v33','extraction-v34','extraction-v35','extraction-v36'],
                        default='extraction-v28')
    parser.add_argument('--execute',action='store_true')
    args=parser.parse_args()
    _,packets=budget.packets()
    selected=[p for p in packets if p['case']['id']==args.case]
    budget.require(len(selected)==1 and args.case in ALLOWED,'not a selected frozen case')
    if not args.execute:
        print(json.dumps(dict(status='dry_run',case=args.case,version=args.version,
            quote=api.quote(body(selected[0],args.version)),
            prompt_sha256=focused.sha(prompt(args.version).encode()))))
        return
    print(json.dumps(dispatch(selected[0],args.evidence,
        json.loads(args.authorization.read_text()),args.version),ensure_ascii=False))

if __name__=='__main__': main()
