"""One targeted versioned reviewer experiment on an immutable v23 candidate per invocation.

Uses the existing Anthropic transport, price bound, accounting ledger and frozen packets.
Never publishes or approves Claims. No retries, fallback, or alternate billing account.
"""
import argparse
import json
import os
from pathlib import Path

import focused_review as focused
import guarded_anthropic as api
import guarded_haiku as budget

SCHEMA_STATUS = {'type':'string','enum':['pass','reject','unresolved']}
SCHEMA_DIMENSION = {'type':'object','additionalProperties':False,'properties':{
    'status':{'type':'string','enum':['pass','reject','unresolved','not_applicable']},
    'reason':{'type':'string'}},'required':['status','reason']}
DIMENSIONS = ('atomicity','entailment','attribution','polarity_scope','missing_argument',
              'unsupported_inference','korean_meaning')
EVIDENCE = {'type':'array','items':{'type':'object','additionalProperties':False,
    'properties':{'quote':{'type':'string'}},'required':['quote']}}
CLAIM = {'type':'object','additionalProperties':False,'properties':{
    'index':{'type':'integer'},'text':{'type':'string'},'evidence':EVIDENCE,
    'dimensions':{'type':'object','additionalProperties':False,
                  'properties':{k:SCHEMA_DIMENSION for k in DIMENSIONS},'required':list(DIMENSIONS)},
    'uncertainty_flags':{'type':'array','items':{'type':'string'}}},
    'required':['index','text','evidence','dimensions','uncertainty_flags']}
INVENTORY = {'type':'object','additionalProperties':False,'properties':{
    'proposition':{'type':'string'},'evidence':EVIDENCE,
    'claim_indices':{'type':'array','items':{'type':'integer'}},'status':SCHEMA_STATUS,
    'reason':{'type':'string'}},
    'required':['proposition','evidence','claim_indices','status','reason']}
SCHEMA = {'type':'object','additionalProperties':False,'properties':{
    'verdict':SCHEMA_STATUS,'uncertainty_flags':{'type':'array','items':{'type':'string'}},
    'claims':{'type':'array','items':CLAIM},'inventory':{'type':'array','items':INVENTORY}},
    'required':['verdict','uncertainty_flags','claims','inventory']}

def schema(version):
    result=json.loads(json.dumps(SCHEMA))
    if version in {'v27','v28'}:
        dimensions=result['properties']['claims']['items']['properties']['dimensions']
        dimensions['properties']['source_meta_expansion']=SCHEMA_DIMENSION
        dimensions['required'].append('source_meta_expansion')
    return result


def body(packet, version):
    protocol=(focused.ROOT/f'prompts/extraction-{version}/review.txt').read_text()
    if version=='v28':
        frames=(focused.ROOT/'prompts/extraction-v28/frames.txt').read_text()
        protocol+='\n'+frames+'\nIndependent review responsibility: semantic fidelity and epistemic framing. Compare against original, never a translation.'
    content=focused.canonical(dict(original_source=packet['case']['source'],
                                   candidate_batch=packet['candidate'])).decode()
    budget.require(len(protocol.encode())+len(content.encode())<=256*1024,'oversized context')
    return dict(model=api.MODEL,max_tokens=6000,system=protocol,
                messages=[dict(role='user',content=content)],
                output_config=dict(effort='low',format=dict(type='json_schema',schema=schema(version))),
                service_tier='standard_only')


def shape(value, rule):
    kind=rule['type']
    budget.require({'object':lambda:isinstance(value,dict),
        'array':lambda:isinstance(value,list),'string':lambda:isinstance(value,str),
        'integer':lambda:type(value) is int}[kind](),'invalid review field type')
    if 'enum' in rule:
        budget.require(value in rule['enum'],'invalid review enum')
    if kind=='object':
        budget.require(set(rule.get('required',[]))<=set(value) and
            (rule.get('additionalProperties',True) or set(value)<=set(rule['properties'])),
            'invalid review fields')
        for key,item in value.items():
            shape(item,rule['properties'][key])
    elif kind=='array':
        for item in value:
            shape(item,rule['items'])


def check(j,packet,version):
    shape(j,schema(version))
    source=packet['case']['source']
    claims=packet['candidate']['claims']
    budget.require(len(j['claims'])==len(claims),'review omitted Claim')
    rejected=False
    budget.require(all(f.strip() for f in j['uncertainty_flags']),'empty uncertainty flag')
    unresolved=bool(j['uncertainty_flags'])
    def unique(quote):
        first=source.find(quote)
        return bool(quote) and first>=0 and source.find(quote,first+1)<0
    for index,(row,claim) in enumerate(zip(j['claims'],claims)):
        budget.require(type(row['index']) is int and row['index']==index and
                       row['text']==claim['text'],'changed Claim identity')
        budget.require(row['evidence'] and all(unique(e['quote'])
                       for e in row['evidence']),'ambiguous/nonexact Claim evidence')
        korean=claim['lang']=='ko' or any('\uac00'<=c<='\ud7a3' for c in source)
        for key in (*DIMENSIONS, *(['source_meta_expansion'] if version in {'v27','v28'} else [])):
            d=row['dimensions'][key]
            budget.require(d['reason'].strip() and d['status'] in
                ({'pass','reject','unresolved','not_applicable'} if key=='korean_meaning' else
                 {'pass','reject','unresolved'}),'invalid dimension')
            budget.require(not (key=='korean_meaning' and korean and d['status']=='not_applicable'),
                           'unassessed Korean dependency')
            rejected |= d['status']=='reject'
            unresolved |= d['status']=='unresolved'
        budget.require(all(f.strip() for f in row['uncertainty_flags']),'empty uncertainty flag')
        unresolved |= bool(row['uncertainty_flags'])
    budget.require(j['inventory'],'empty source proposition inventory')
    for row in j['inventory']:
        budget.require(row['proposition'].strip() and row['reason'].strip() and row['evidence']
            and all(unique(e['quote']) for e in row['evidence']),
            'invalid inventory evidence')
        budget.require(all(type(i) is int and 0<=i<len(claims) for i in row['claim_indices']),
                       'invalid inventory Claim index')
        budget.require(len(set(row['claim_indices']))==len(row['claim_indices']),'duplicate inventory Claim index')
        budget.require(row['status']!='pass' or row['claim_indices'],'uncovered passing inventory')
        rejected |= row['status']=='reject'
        unresolved |= row['status']=='unresolved'
    expected='reject' if rejected else ('unresolved' if unresolved else 'pass')
    budget.require(j['verdict']==expected,'contradictory aggregate verdict')
    return dict(protocol_valid=True,verdict=j['verdict'],claim_count=len(claims),
                inventory_count=len(j['inventory']),semantic_acceptance=None,
                production_approval=False)


def dispatch(packet, evidence, authorization, version, label=''):
    ident='focused-haiku-repair-'+version+'-'+packet['case']['id']+label+'-primary'
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
            packet_sha256=focused.sha(focused.canonical(packet)),prompt_sha256=focused.sha(
            (focused.ROOT/f'prompts/extraction-{version}/review.txt').read_bytes()),
            system_sha256=focused.sha(request['system'].encode()),
            model_info=model,quote=quote,authorization=authorization))
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
        budget.require(response['stop_reason']=='end_turn','incomplete model response')
        raw=''.join(b['text'] for b in response['content'] if b.get('type')=='text')
        j=focused.parse_judgment(raw)
        try:
            result=check(j,packet,version)
        except (ValueError,KeyError,TypeError) as error:
            budget.immutable(evidence/(ident+'.rejection.json'),dict(status='protocol_invalid',
                reason=str(error),model_verdict=j.get('verdict'),semantic_acceptance=None,
                production_approval=False))
            return dict(id=ident,status='protocol_invalid',reason=str(error))
        budget.immutable(evidence/(ident+'.judgment.json'),j)
        budget.immutable(evidence/(ident+'.outcome.json'),result)
        return dict(id=ident,**result)
    finally:
        conn.close()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--case',required=True)
    parser.add_argument('--evidence',type=Path,required=True)
    parser.add_argument('--authorization',type=Path,required=True)
    parser.add_argument('--version',choices=['v24','v25','v27','v28'],default='v28')
    selection=parser.add_mutually_exclusive_group()
    selection.add_argument('--candidate-version',choices=['extraction-v25','extraction-v26','extraction-v28'])
    selection.add_argument('--control',choices=['nested-perspective'])
    parser.add_argument('--replicate',choices=['primary','repeat'],default='primary')
    parser.add_argument('--execute',action='store_true')
    args=parser.parse_args()
    _,packets=budget.packets()
    selected=[p for p in packets if p['case']['id']==args.case]
    budget.require(len(selected)==1 and args.case in {'r1-nested_quote','v23-fresh-003','r1-ambiguous_ref','r1-hypothetical','r1-habitual_temporal','v23-fresh-004'},
                   'only frozen priority cases permitted')
    packet=selected[0]
    label=''
    if args.candidate_version:
        path=args.evidence/('focused-haiku-'+args.candidate_version+'-'+args.case+'-primary.candidate.json')
        original=args.evidence/('focused-haiku-'+args.candidate_version+'-'+args.case+'-primary.request.json')
        budget.require(path.is_file() and original.is_file(),'saved extraction evidence required')
        request=json.loads(original.read_text())
        budget.require(request['source_sha256']==focused.sha(packet['case']['source'].encode()),
                       'candidate source mismatch')
        packet=dict(packet,candidate=json.loads(path.read_text()),
                    derived_candidate_sha256=focused.sha(path.read_bytes()))
        label='-'+args.candidate_version
    if args.control:
        path=Path(__file__).with_name('semantic_review_controls.json')
        control=json.loads(path.read_text())[args.control]
        budget.require(control['case']==args.case and control['source_sha256']==focused.sha(packet['case']['source'].encode()),'control source mismatch')
        packet=dict(packet,candidate=control['candidate'],control_authorship=control['authorship'],
            control_sha256=focused.sha(focused.canonical(control)))
        label='-control-'+args.control
    if args.replicate=='repeat':
        label+='-repeat'
    if not args.execute:
        print(json.dumps(dict(status='dry_run',case=args.case,version=args.version,label=label,
                              quote=api.quote(body(packet,args.version)))))
        return
    print(json.dumps(dispatch(packet,args.evidence,
        json.loads(args.authorization.read_text()),args.version,label),ensure_ascii=False))

if __name__=='__main__': main()
