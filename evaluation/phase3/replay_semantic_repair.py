"""Read-only replay of saved diagnostic requests; no model calls or publication.

Original receipts remain immutable. Rust replays structural validation only; absent
full Plan/reader/three-review evidence never becomes experimental gate eligibility.
"""
import argparse
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile

import focused_review as focused
import guarded_anthropic as api
import guarded_anthropic_extraction as extraction
import guarded_anthropic_repair as repair
import guarded_haiku as budget


def load(path):
    return focused.parse_judgment(path.read_text())


def raw_text(response):
    return ''.join(b['text'] for b in response['content'] if b.get('type')=='text')


def signature(j):
    return dict(verdict=j['verdict'],uncertainty=j['uncertainty_flags'],claims=[dict(
        index=c['index'],evidence=c['evidence'],uncertainty=c['uncertainty_flags'],
        dimensions={k:v['status'] for k,v in c['dimensions'].items()}) for c in j['claims']],
        inventory=[dict(proposition=i['proposition'],evidence=i['evidence'],claim_indices=([i['claim_index']] if i['claim_index'] is not None else []) if 'claim_index' in i else i['claim_indices'],status=i['status']) for i in j['inventory']])


def request_identity(body):
    # Repeat trials must also share model, effort, output cap, schema and tier.
    return focused.sha(focused.canonical(body))


def replay(evidence):
    packets=budget.packets()[1]
    sources={p['case']['source']:p['case'] for p in packets}
    source_hashes={focused.sha(s.encode()):case for s,case in sources.items()}
    rows=[]
    structural=[]
    repeated={}
    for path in sorted(evidence.glob('focused-haiku-*.request.json')):
        request=load(path)
        ident=request['id']
        response_path=evidence/(ident+'.response.json')
        if not response_path.is_file():
            body=request['payload'];payload=focused.parse_judgment(body['messages'][0]['content'])
            reader=request.get('reader_index') is not None
            case=source_hashes[request['source_sha256']]
            version=ident.split('-')[3]
            kind='reader' if reader else 'review'
            if ident.startswith('focused-haiku-extraction-'):
                version='extraction-'+version
                packet=dict(case=case)
                if request.get('repair_context') is not None:
                    packet['repair_context']=request['repair_context']
                expected=extraction.body(packet,version,request.get('output_format','legacy'),request.get('extraction_output_tokens'))
                kind='extraction'
            else:
                expected=(repair.reader_body(payload['claim_text'],version) if reader else
                    repair.body(dict(case=case,candidate=payload['candidate_batch']),version,
                        request.get('review_stage',repair.STAGES['fidelity']),request.get('review_effort','low'),
                        request.get('review_output_tokens'),request.get('schema_revision','unsupported-v1') if version in {'v39','v40','v41','v42','v43','v44','v45'} else None))
            budget.require(body==expected and api.quote(body)==request['quote'],'pending request recipe drift')
            api.verify_request_serialization(request,expected,api.request_mode(version))
            error_path=evidence/(ident+'.transport-error.json')
            rows.append(dict(id=ident,case=case['id'],kind=kind,version=version,
                complete=False,protocol_valid=False,pending_reservation=True,model_response=False,
                conservative_upper_nanos=request['quote']['estimate'],
                request_sha256=focused.sha(path.read_bytes()),payload_sha256=request_identity(body),
                transport_error=load(error_path) if error_path.exists() else None,
                failure_reason='no model response; reservation retained; no completion inferred',production_approval=False))
            continue
        response=load(response_path)
        usage=api.conservative_usage(response)
        budget.require(usage==load(evidence/(ident+'.accounting.json')),'accounting drift')
        body=request['payload']
        budget.require(request.get('review_effort',body['output_config']['effort'])==
                       body['output_config']['effort'],'effort receipt drift')
        payload=focused.parse_judgment(body['messages'][0]['content'])
        reader=request.get('reader_index') is not None
        case=(source_hashes[request['source_sha256']] if reader else
              sources[payload['original_source']])
        source=case['source']
        row=dict(id=ident,case=case['id'],request_sha256=focused.sha(path.read_bytes()),
            payload_sha256=request_identity(body),
            effort=body['output_config']['effort'],output_token_cap=body['max_tokens'],
            response_sha256=focused.sha(response_path.read_bytes()),
            system_sha256=focused.sha(body['system'].encode()),
            input_sha256=focused.sha(body['messages'][0]['content'].encode()),
            conservative_upper_nanos=usage['conservative_upper_nanos'])
        rejected=(evidence/(ident+'.rejection.json')).is_file()
        saved=load(evidence/(ident+('.rejection.json' if rejected else '.outcome.json')))
        budget.require(saved['production_approval'] is False and saved['semantic_acceptance'] is None,
                       'diagnostic boundary changed')
        if response['stop_reason']!='end_turn':
            version=ident.split('-')[3]
            kind='reader' if reader else 'review'
            if ident.startswith('focused-haiku-extraction-'):
                version='extraction-'+version;kind='extraction'
                packet=dict(case=case)
                if request.get('repair_context') is not None:packet['repair_context']=request['repair_context']
                expected=extraction.body(packet,version,request.get('output_format','legacy'),request.get('extraction_output_tokens'))
            else:
                expected=(repair.reader_body(payload['claim_text'],version) if reader else
                    repair.body(dict(case=case,candidate=payload['candidate_batch']),version,
                        request.get('review_stage',repair.STAGES['fidelity']),request.get('review_effort','low'),request.get('review_output_tokens'),request.get('schema_revision','unsupported-v1') if version in {'v39','v40','v41','v42','v43','v44','v45'} else None))
            budget.require(body==expected,'incomplete request recipe drift')
            api.verify_request_serialization(request,expected,api.request_mode(version))
            reason='stop_reason:'+str(response['stop_reason'])
            budget.require(rejected and saved['status']=='incomplete' and saved['reason']==reason,
                           'incomplete rejection drift')
            row.update(kind=kind,version=version,protocol_valid=False,complete=False,
                       failure_reason=reason)
            if kind=='review':
                repeated.setdefault(row['payload_sha256'],[]).append((ident,dict(
                    verdict=None,protocol_valid=False,failure_reason=reason)))
            rows.append(row)
            continue
        if ident.startswith('focused-haiku-extraction-'):
            version=ident.split('-')[3]
            version='extraction-'+version
            budget.require(body['system']==extraction.prompt(version),'extraction prompt drift')
            budget.require(request['source_sha256']==focused.sha(source.encode()),'Source drift')
            recipe_packet=dict(case=case)
            if request.get('repair_context') is not None:
                recipe_packet['repair_context']=request['repair_context']
            expected=extraction.body(recipe_packet,version,request.get('output_format','legacy'),request.get('extraction_output_tokens'))
            budget.require(body==expected,'extraction request recipe drift')
            api.verify_request_serialization(request,expected,api.request_mode(version))
            try:
                result=extraction.structural(raw_text(response),source)
            except (ValueError,KeyError,TypeError) as error:
                budget.require(rejected and saved['status']=='structural_invalid' and
                    saved['reason']==str(error),'extraction rejection replay drift')
                row.update(kind='extraction',version=version,claim_count=None,
                    local_structure_valid=False,failure_reason=str(error))
            else:
                budget.require(not rejected and result==load(evidence/(ident+'.candidate.json')),'candidate drift')
                row.update(kind='extraction',version=version,claim_count=len(result['claims']),
                           local_structure_valid=True)
            structural.append(dict(id=ident,corpus_case=case,source_hash=focused.sha(source.encode()),
                raw=raw_text(response),variant='audited',plan=None,reviews=[]))
        elif reader:
            version=ident.split('-')[3]
            expected=repair.reader_body(payload['claim_text'],version)
            budget.require(body==expected,'reader request drift')
            api.verify_request_serialization(request,expected,api.request_mode(version))
            judgment=focused.parse_judgment(raw_text(response))
            checked=repair.reader_check(judgment)
            budget.require(checked==saved,'reader replay drift')
            budget.require(judgment==load(evidence/(ident+'.judgment.json')),'reader judgment drift')
            row.update(kind='reader',version=version,**checked)
        else:
            version=ident.split('-')[3]
            packet=dict(case=case,candidate=payload['candidate_batch'])
            expected=repair.body(packet,version,request.get('review_stage',repair.STAGES['fidelity']),
                                 request.get('review_effort','low'),request.get('review_output_tokens'),request.get('schema_revision','unsupported-v1') if version in {'v39','v40','v41','v42','v43','v44','v45'} else None)
            budget.require(body==expected,'review request recipe drift')
            api.verify_request_serialization(request,expected,api.request_mode(version))
            judgment=focused.parse_judgment(raw_text(response))
            try:
                checked=repair.check(judgment,packet,version,
                    strict_entailment=request.get('review_validation_version') in {'source-checks-v5','source-checks-v6'} and version in {'v34','v35','v36','v37','v38'},
                    strict_bijection=request.get('review_validation_version')=='source-checks-v6',stage=request.get('review_stage'))
            except (ValueError,KeyError,TypeError) as error:
                budget.require(rejected and saved['status']=='protocol_invalid' and
                    saved['reason']==str(error) and saved['model_verdict']==judgment.get('verdict'),
                    'rejection replay drift')
                row.update(kind='review',version=version,protocol_valid=False,
                    failure_reason=str(error),model_verdict=judgment.get('verdict'))
                key=row['payload_sha256']
                repeated.setdefault(key,[]).append((ident,dict(verdict=judgment.get('verdict'),
                    protocol_valid=False,failure_reason=str(error))))
                rows.append(row)
                continue
            budget.require(not rejected,'saved rejection unexpectedly passed')
            budget.require(checked==saved,'protocol replay drift')
            budget.require(judgment==load(evidence/(ident+'.judgment.json')),'judgment drift')
            row.update(kind='review',version=version,**checked)
            try:
                repair.check(judgment,packet,version,stage=request.get('review_stage'))
                row['current_protocol_valid']=True
            except (ValueError,KeyError,TypeError) as error:
                row.update(current_protocol_valid=False,current_protocol_error=str(error))
            row['historical_classification_preserved']=True
            key=row['payload_sha256']
            repeated.setdefault(key,[]).append((ident,signature(judgment)))
        rows.append(row)
    with tempfile.TemporaryDirectory(prefix='prime-radiant-semantic-replay-') as temp:
        src=Path(temp)/'input.jsonl'
        dst=Path(temp)/'output.jsonl'
        src.write_text(''.join(json.dumps(r,ensure_ascii=False)+'\n' for r in structural))
        completed=subprocess.run(['cargo','run','--offline','--quiet','--example',
            'revalidate_extraction','--','--input',str(src),'--output',str(dst)],
            cwd=focused.ROOT,capture_output=True,text=True)
        budget.require(completed.returncode==0,'Rust structural replay failed: '+completed.stderr)
        results={r['id']:r['revalidation'] for r in map(json.loads,dst.read_text().splitlines())}
        for row in rows:
            if row['kind']=='extraction' and row['id'] in results:
                row['rust_revalidation']=results[row['id']]
                budget.require(not results[row['id']]['experimental_gate_eligible'],
                               'diagnostic request credited as gate eligible')
    comparisons=[]
    for group in repeated.values():
        if len(group)<2:
            continue
        base=group[0][1]
        comparisons.append(dict(ids=[g[0] for g in group],
            verdict_agreement=all(g[1]['verdict']==base['verdict'] for g in group),
            complete_signature_agreement=all(g[1]==base for g in group),
            signatures=[g[1] for g in group]))
    conn=sqlite3.connect('file:'+str(budget.LEDGER)+'?mode=ro',uri=True)
    try:
        ceiling,prior=conn.execute('SELECT ceiling,prior FROM budget').fetchone()
        charge,unsettled,count=conn.execute('SELECT SUM(COALESCE(actual,estimate)),SUM(actual IS NULL),COUNT(*) FROM request').fetchone()
        for row in rows:
            actual=conn.execute('SELECT actual,estimate FROM request WHERE id=?',(row['id'],)).fetchone()
            if row.get('pending_reservation'):
                budget.require(actual and actual[0] is None and actual[1]==row['conservative_upper_nanos'],'pending reservation mismatch')
            else:budget.require(actual and actual[0]==row['conservative_upper_nanos'],'ledger receipt mismatch')
    finally:
        conn.close()
    return dict(status='diagnostic_replay_only',independent_semantic_ground_truth=False,
        production_approvals=0,rows=rows,repeat_comparisons=comparisons,
        extraction_requests=sum(r['kind']=='extraction' for r in rows),
        distinct_extraction_sources=len({r['case'] for r in rows if r['kind']=='extraction'}),
        review_requests=sum(r['kind']=='review' for r in rows),
        reader_requests=sum(r['kind']=='reader' for r in rows),
        ledger=dict(ceiling_nanos=ceiling,prior_nanos=prior,recorded_upper_nanos=charge,
            cumulative_upper_nanos=prior+charge,remaining_nanos=ceiling-prior-charge,
            unsettled=unsettled,requests=count,provider_billed_usd=None))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    result=replay(args.evidence)
    budget.immutable(args.output,result)
    print(json.dumps({k:v for k,v in result.items() if k not in {'rows','repeat_comparisons'}}))

if __name__=='__main__':
    main()
