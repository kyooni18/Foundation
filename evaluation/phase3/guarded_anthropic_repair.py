"""One targeted versioned reviewer experiment on an immutable v23 candidate per invocation.

Uses the existing Anthropic transport, price bound, accounting ledger and frozen packets.
Never publishes or approves Claims. No retries, fallback, or alternate billing account.
"""
import argparse
import json
import os
import urllib.error
from pathlib import Path

import focused_review as focused
import guarded_anthropic as api
import guarded_anthropic_extraction as extraction
import faceted_review
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
    if version in {'v27','v28','v29','v30','v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'}:
        dimensions=result['properties']['claims']['items']['properties']['dimensions']
        dimensions['properties']['source_meta_expansion']=SCHEMA_DIMENSION
        dimensions['required'].append('source_meta_expansion')
    if version in {'v30','v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'}:
        dimensions=result['properties']['claims']['items']['properties']['dimensions']
        dimensions['properties']['duplicates']=SCHEMA_DIMENSION
        dimensions['required'].append('duplicates')
    return result


STAGES={'atomicity':'atomicity and independence','fidelity':'semantic fidelity and epistemic framing',
        'coverage':'whole-batch coverage, duplication and conflicting interpretation'}


def stages(version):
    return faceted_review.stages(version) if version in {'v39','v40','v41','v42','v43','v44'} else STAGES

def body(packet, version, stage=None, effort='low', output_tokens=None, schema_revision=None):
    stage=stage or stages(version)['fidelity']
    if version in {'v39','v40','v41','v42','v43','v44'}:
        budget.require(effort=='low' and output_tokens in {None,6000},'unsupported faceted configuration')
        return faceted_review.body(packet,stage,api.MODEL,schema_revision or ('supported-v3' if version in {'v43','v44'} else 'supported-v2'),version)
    budget.require(output_tokens is None or (version in {'v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'} and output_tokens==6000), 'unsupported review response budget')
    budget.require(effort=='low' or (effort=='medium' and version=='v31'),
                   'unsupported diagnostic effort')
    protocol=(focused.ROOT/f'prompts/extraction-{version}/review.txt').read_text()
    if version=='v28':
        frames=(focused.ROOT/'prompts/extraction-v28/frames.txt').read_text()
        protocol+='\n'+frames+'\nIndependent review responsibility: semantic fidelity and epistemic framing. Compare against original, never a translation.'
    if version in {'v29','v30','v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'}:
        protocol+=f'\nIndependent review responsibility: {stage}. Compare against original, never a translation.'
    payload=dict(original_source=packet['case']['source'],candidate_batch=packet['candidate'])
    if version in {'v29','v30','v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'}:
        payload.update(authored_at=packet['case'].get('authored_at'),candidate_count=len(packet['candidate']['claims']),
            required_checked_indices=list(range(len(packet['candidate']['claims']))))
    content=focused.canonical(payload).decode()
    budget.require(len(protocol.encode())+len(content.encode())<=256*1024,'oversized context')
    output_schema=schema(version)
    if version=='v38' and (focused.has_korean(packet['case']['source']) or
            any(c.get('lang')=='ko' for c in packet['candidate']['claims'])):
        output_schema['properties']['claims']['items']['properties']['dimensions']['properties']['korean_meaning']['properties']['status']={'type':'string','enum':['pass','reject','unresolved']}
    if version in {'v29','v30','v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'}:
        for section in ('claims','inventory'):
            output_schema['properties'][section]['items']['properties']['evidence']['items']['properties']['quote']['enum']=[packet['case']['source']]
    if version in {'v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'} and packet['candidate']['claims']:
        output_schema['properties']['claims']['minItems']=1
        output_schema['properties']['inventory']['minItems']=1
    return dict(model=api.MODEL,max_tokens=output_tokens or (6000 if version in {'v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'} else 3000 if version in {'v29','v30','v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'} else 6000),system=protocol,
                messages=[dict(role='user',content=content)],
                output_config=dict(effort=effort,format=dict(type='json_schema',schema=output_schema)),
                service_tier='standard_only')


def shape(value, rule):
    kind=rule['type']
    budget.require({'object':lambda:isinstance(value,dict),
        'array':lambda:isinstance(value,list),'string':lambda:isinstance(value,str),
        'integer':lambda:type(value) is int,'boolean':lambda:type(value) is bool}[kind](),'invalid review field type')
    if 'enum' in rule:
        budget.require(value in rule['enum'],'invalid review enum')
    if kind=='object':
        budget.require(set(rule.get('required',[]))<=set(value) and
            (rule.get('additionalProperties',True) or set(value)<=set(rule['properties'])),
            'invalid review fields')
        for key,item in value.items():
            shape(item,rule['properties'][key])
    elif kind=='array':
        budget.require(len(value)>=rule.get('minItems',0),'array below minimum length')
        for item in value:
            shape(item,rule['items'])


def check(j,packet,version,strict_entailment=None,strict_bijection=True,stage=None):
    if version in {'v39','v40','v41','v42','v43','v44'}:
        return faceted_review.check(j,packet,stage,shape,version)
    if strict_entailment is None:
        strict_entailment=version in {'v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'}
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
        if version in {'v29','v30','v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'}:
            budget.require(row['evidence']==[dict(quote=source)],'complete original context required')
        korean=claim['lang']=='ko' or focused.has_korean(source)
        for key in (*DIMENSIONS, *(['source_meta_expansion'] if version in {'v27','v28','v29','v30','v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'} else []), *(['duplicates'] if version in {'v30','v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'} else [])):
            d=row['dimensions'][key]
            budget.require(d['reason'].strip() and d['status'] in
                ({'pass','reject','unresolved','not_applicable'} if key=='korean_meaning' else
                 {'pass','reject','unresolved'}),'invalid dimension')
            budget.require(not (key=='korean_meaning' and korean and d['status']=='not_applicable'),
                           'unassessed Korean dependency')
            rejected |= d['status']=='reject'
            unresolved |= d['status']=='unresolved'
        dimensions=row['dimensions']
        budget.require(not (strict_entailment and dimensions['entailment']['status']=='pass' and
            any(dimensions.get(k,{}).get('status')=='reject' for k in ['unsupported_inference','source_meta_expansion'])),
            'contradictory complete-claim entailment')
        budget.require(all(f.strip() for f in row['uncertainty_flags']),'empty uncertainty flag')
        unresolved |= bool(row['uncertainty_flags'])
    empty_reason=packet['candidate'].get('empty_reason')
    budget.require(j['inventory'] or (not claims and isinstance(empty_reason,str) and empty_reason.strip()),
                   'empty inventory requires a valid empty extraction')
    covered=set()
    inventory_entries=set()
    for row in j['inventory']:
        budget.require(row['proposition'].strip() and row['reason'].strip() and row['evidence']
            and all(unique(e['quote']) for e in row['evidence']),
            'invalid inventory evidence')
        if version in {'v29','v30','v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'}:
            budget.require(row['evidence']==[dict(quote=source)] and unique(row['proposition']),'canonical original inventory required')
        budget.require(all(type(i) is int and 0<=i<len(claims) for i in row['claim_indices']),
                       'invalid inventory Claim index')
        budget.require(len(set(row['claim_indices']))==len(row['claim_indices']),'duplicate inventory Claim index')
        identity=(row['proposition'],tuple(sorted({e['quote'] for e in row['evidence']})),
                  tuple(sorted(row['claim_indices'])))
        budget.require(identity not in inventory_entries,'duplicate source inventory entry')
        inventory_entries.add(identity)
        budget.require(row['status']!='pass' or row['claim_indices'],'uncovered passing inventory')
        if row['status']=='pass':
            covered.update(row['claim_indices'])
        rejected |= row['status']=='reject'
        unresolved |= row['status']=='unresolved'
    expected='reject' if rejected else ('unresolved' if unresolved else 'pass')
    budget.require(j['verdict']==expected,'contradictory aggregate verdict')
    budget.require(expected!='pass' or covered==set(range(len(claims))),'passing inventory omitted a candidate Claim')
    if strict_bijection and expected=='pass':
        mappings=[r['claim_indices'] for r in j['inventory']]
        propositions=[r['proposition'] for r in j['inventory']]
        budget.require(len(mappings)==len(claims) and all(len(m)==1 for m in mappings) and
            len({m[0] for m in mappings})==len(claims) and len(set(propositions))==len(propositions),
            'passing inventory must map propositions and Claims one-to-one')
    return dict(protocol_valid=True,verdict=j['verdict'],claim_count=len(claims),
                inventory_count=len(j['inventory']),semantic_acceptance=None,
                production_approval=False)


READER_SCHEMA={'type':'object','additionalProperties':False,'properties':{
    'verdict':SCHEMA_STATUS,'issues':{'type':'array','items':{'type':'string',
        'enum':['atomicity','oversplit','reference','temporal','ambiguity']}},
    'checked':{'type':'array','items':{'type':'integer'}},'coverage_complete':{'type':'boolean'}},
    'required':['verdict','issues','checked','coverage_complete']}


def reader_body(text,version):
    reader_schema=READER_SCHEMA
    if version=='v44':
        reader_schema=dict(READER_SCHEMA,properties={k:READER_SCHEMA['properties'][k] for k in ['checked','coverage_complete','issues','verdict']})
    return dict(model=api.MODEL,max_tokens=6000 if version in {'v39','v40','v41','v42','v43','v44'} else 3000,
        system=(focused.ROOT/f'prompts/extraction-{version}/reader.txt').read_text(),
        messages=[dict(role='user',content=focused.canonical(dict(claim_text=text,candidate_count=1,required_checked_indices=[0])).decode())],
        output_config=dict(effort='low',format=dict(type='json_schema',schema=reader_schema)),
        service_tier='standard_only')


def reader_check(j):
    shape(j,READER_SCHEMA)
    budget.require(j['checked']==[0] and j['coverage_complete'],'incomplete reader inspection')
    budget.require(j['verdict']!='pass' or not j['issues'],'contradictory reader pass')
    return dict(protocol_valid=True,verdict=j['verdict'],claim_count=1,inventory_count=0,
        semantic_acceptance=None,production_approval=False)


def dispatch(packet,evidence,authorization,*args,**kwargs):
    with budget.dispatch_slot(budget.LEDGER,authorization):
        return _dispatch(packet,evidence,authorization,*args,**kwargs)

def _dispatch(packet, evidence, authorization, version, label='', stage=STAGES['fidelity'], reader_index=None, effort='low', output_tokens=None):
    budget.require(effort=='low' or (effort=='medium' and version=='v31' and reader_index is None),
                   'unsupported diagnostic effort')
    if version in {'v39','v40','v41','v42','v43','v44'} and stage==STAGES['fidelity']:
        stage=stages(version)['fidelity']
    if effort!='low':
        label+='-effort-'+effort
    if output_tokens is not None:
        budget.require(reader_index is None, 'Source review response budget only')
        label+='-output-'+str(output_tokens)
    if version in {'v39','v40','v41','v42','v43','v44'} and reader_index is None:
        label+='-schema-v3' if version in {'v43','v44'} else '-schema-v2'
    ident='focused-haiku-repair-'+version+'-'+packet['case']['id']+label+'-primary'
    key=os.environ.get('ANTHROPIC_API_KEY')
    budget.require(key and len(key)<1024 and '\n' not in key and '\r' not in key,
                   'ANTHROPIC_API_KEY not configured')
    mode=api.request_mode(version)
    d=api.Dispatcher(budget.LEDGER,evidence,api.transport(key,mode),authorization)
    conn=d.connection()
    try:
        d.check(conn,1)
        budget.require(conn.execute('SELECT 1 FROM request WHERE id=?',(ident,)).fetchone() is None,
                       'already attempted; no retry')
        request=(body(packet,version,stage,effort,output_tokens) if reader_index is None else
                 reader_body(packet['candidate']['claims'][reader_index]['text'],version))
        quote=api.quote(request)
        d.check(conn,quote['estimate'])
        model=d.transport('GET','/v1/models/'+api.MODEL,None)
        budget.require(model.get('id')==api.MODEL,'unexpected model identity')
        conn.execute('BEGIN IMMEDIATE')
        _,cumulative=d.check(conn,quote['estimate'])
        conn.execute('INSERT INTO request(id,model,created_at,estimate,cumulative) '
                     "VALUES(?,?,datetime('now'),?,?)",
                     (ident,'anthropic/'+api.MODEL,quote['estimate'],cumulative))
        budget.immutable(evidence/(ident+'.request.json'),dict(id=ident,payload=request,transport_serialization=mode,wire_sha256=focused.sha(api.serialize_request(request,mode)),
            packet_sha256=focused.sha(focused.canonical(packet)),
            source_sha256=focused.sha(packet['case']['source'].encode()),
            schema_revision=('supported-v3' if version in {'v43','v44'} else 'supported-v2') if version in {'v39','v40','v41','v42','v43','v44'} else None,review_validation_version='source-checks-v6',review_stage=stage,review_effort=effort,review_output_tokens=output_tokens,reader_index=reader_index,prompt_sha256=focused.sha(
            (focused.ROOT/f"prompts/extraction-{version}/{'review' if reader_index is None else 'reader'}.txt").read_bytes()),
            system_sha256=focused.sha(request['system'].encode()),
            model_info=model,quote=quote,authorization=authorization))
        conn.commit()
        try:
            response=d.transport('POST','/v1/messages',request)
        except (urllib.error.HTTPError,urllib.error.URLError,TimeoutError) as error:
            detail=None
            if isinstance(error,urllib.error.HTTPError):
                with error:
                    raw=error.read(4*1024*1024+1)
                detail=raw[:4*1024*1024].decode('utf-8',errors='replace').replace(key,'[REDACTED_API_KEY]')
            budget.immutable(evidence/(ident+'.transport-error.json'),dict(
                status=getattr(error,'code',None),error_type=type(error).__name__,response_body=detail,
                reservation_retained=True,production_approval=False))
            raise ValueError('provider transport failure; reservation retained; no automatic retry') from None
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
        if response['stop_reason']!='end_turn':
            result=dict(status='incomplete',reason='stop_reason:'+str(response['stop_reason']),
                semantic_acceptance=None,production_approval=False)
            budget.immutable(evidence/(ident+'.rejection.json'),result)
            return dict(id=ident,**result)
        raw=''.join(b['text'] for b in response['content'] if b.get('type')=='text')
        j=focused.parse_judgment(raw)
        try:
            result=(check(j,packet,version,stage=stage) if reader_index is None else reader_check(j))
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
    parser.add_argument('--version',choices=['v24','v25','v27','v28','v29','v30','v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'],default='v28')
    selection=parser.add_mutually_exclusive_group()
    selection.add_argument('--candidate-version',choices=['extraction-v25','extraction-v26','extraction-v28','extraction-v29','extraction-v30','extraction-v31','extraction-v32','extraction-v33','extraction-v34','extraction-v35','extraction-v36','extraction-v37','extraction-v38','extraction-v39','extraction-v40','extraction-v41','extraction-v42','extraction-v43','extraction-v44','extraction-v41-repair','extraction-v41-schema-v1','extraction-v41-schema-v1-repair','extraction-v42-schema-v1','extraction-v42-schema-v1-repair','extraction-v42-repair','extraction-v43-schema-v2','extraction-v43-schema-v2-repair','extraction-v43-repair','extraction-v44-schema-v3','extraction-v44-schema-v3-repair','extraction-v44-repair'])
    selection.add_argument('--control',choices=['nested-perspective','hypothetical-source-identity','hypothetical-source-role'])
    parser.add_argument('--stage',choices=list(dict(STAGES,**faceted_review.stages())),default='fidelity')
    parser.add_argument('--reader-index',type=int)
    parser.add_argument('--replicate',choices=['primary','repeat'],default='primary')
    parser.add_argument('--effort',choices=['low','medium'],default='low',
                        help='medium is a separately identified v31 source-review diagnostic only')
    parser.add_argument('--output-tokens',type=int,choices=[6000])
    parser.add_argument('--execute',action='store_true')
    args=parser.parse_args()
    budget.require(args.effort=='low' or (args.version=='v31' and args.reader_index is None),
                   'medium diagnostic requires v31 source review')
    budget.require(args.stage in stages(args.version),'stage outside selected profile')
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
        path=Path(__file__).with_name('boundary_review_controls.json' if args.control in {'hypothetical-source-identity','hypothetical-source-role'} else 'semantic_review_controls.json')
        control=json.loads(path.read_text())[args.control]
        budget.require(control['case']==args.case and control['source_sha256']==focused.sha(packet['case']['source'].encode()),'control source mismatch')
        packet=dict(packet,candidate=control['candidate'],control_authorship=control['authorship'],
            control_sha256=focused.sha(focused.canonical(control)))
        label='-control-'+args.control
    if args.replicate=='repeat':
        label+='-repeat'
    budget.require(args.version in {'v29','v30','v31','v32','v33','v34','v35','v36','v37','v38','v39','v40','v41','v42','v43','v44'} or (args.stage=='fidelity' and args.reader_index is None),'new stages require v29')
    if args.reader_index is not None:
        budget.require(0<=args.reader_index<len(packet['candidate']['claims']),'invalid reader index')
        label+='-reader-'+str(args.reader_index)
    elif args.stage!='fidelity':
        label+='-'+args.stage
    budget.require(extraction.rust_structure(packet['candidate'],packet['case'])['valid'],
                   'candidate failed Rust structural validation; no paid review')
    request=(body(packet,args.version,stages(args.version)[args.stage],args.effort,args.output_tokens) if args.reader_index is None else
             reader_body(packet['candidate']['claims'][args.reader_index]['text'],args.version))
    if not args.execute:
        print(json.dumps(dict(status='dry_run',case=args.case,version=args.version,label=label,
                              effort=args.effort,
                              quote=api.quote(request))))
        return
    print(json.dumps(dispatch(packet,args.evidence,
        json.loads(args.authorization.read_text()),args.version,label,stages(args.version)[args.stage],args.reader_index,args.effort,args.output_tokens),ensure_ascii=False))

if __name__=='__main__': main()
