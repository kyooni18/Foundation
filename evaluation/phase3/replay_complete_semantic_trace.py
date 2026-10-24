"""Replay a saved failed prefix or complete extraction/reader/review trace.

Configuration is reconstructed from immutable API requests, not a previously
quarantined production Plan. Eligibility is diagnostic; no approval/publication.
Missing, changed or incomplete evidence fails rather than inventing a review.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

import focused_review as f
import guarded_anthropic as api
import guarded_anthropic_extraction as extraction
import guarded_anthropic_repair as review
import guarded_haiku as budget


def read(path):
    return f.parse_judgment(path.read_text())


def response_text(path):
    value=read(path)
    budget.require(value['stop_reason']=='end_turn','incomplete saved response')
    return ''.join(b['text'] for b in value['content'] if b.get('type')=='text')


def replay(evidence,case_id,version='extraction-v29',candidate_version=None,reader_version=None,review_output_tokens=None):
    packet=next(p for p in budget.packets()[1] if p['case']['id']==case_id)
    budget.require(version in {'extraction-v29','extraction-v30','extraction-v31','extraction-v32','extraction-v33','extraction-v34','extraction-v35','extraction-v36','extraction-v37','extraction-v38','extraction-v39'},'unsupported trace version')
    review_version=version.removeprefix('extraction-')
    candidate_version=candidate_version or version
    reader_version=reader_version or version.removeprefix('extraction-')
    ident=f'focused-haiku-{candidate_version}-{case_id}-primary'
    request=read(evidence/(ident+'.request.json'))
    if request.get('repair_context') is not None:
        packet=dict(packet,repair_context=request['repair_context'])
    budget.require(request['payload']==extraction.body(packet,version),'extraction request drift')
    raw=response_text(evidence/(ident+'.response.json'))
    candidate=extraction.structural(raw,packet['case']['source'])
    packet=dict(packet,candidate=candidate)
    budget.require(candidate==read(evidence/(ident+'.candidate.json')),'candidate sidecar drift')
    reviews=[]
    provenance=[ident]
    prefix=f'focused-haiku-repair-{review_version}-{case_id}-{candidate_version}'
    reader_prefix=f'focused-haiku-repair-{reader_version}-{case_id}-{candidate_version}'
    stop_stage=None
    for index,claim in enumerate(candidate['claims']):
        reader_id=reader_prefix+f'-reader-{index}-primary'
        saved=read(evidence/(reader_id+'.request.json'))
        budget.require(saved['payload']==review.reader_body(claim['text'],review_version),'reader request drift')
        budget.require(saved['reader_index']==index,'reader identity drift')
        text=response_text(evidence/(reader_id+'.response.json'))
        checked=review.reader_check(f.parse_judgment(text))
        reviews.append(dict(stage=f'standalone-{index}',raw=text))
        provenance.append(reader_id)
        if checked['verdict']!='pass':
            stop_stage=f'standalone-{index}'
            break
    for key,stage in review.stages(review_version).items():
        if stop_stage is not None:
            break
        source_id=prefix+('' if key=='fidelity' else '-'+key)+(('-output-'+str(review_output_tokens)) if review_output_tokens else '')+('-schema-v2' if review_version=='v39' else '')+'-primary'
        saved=read(evidence/(source_id+'.request.json'))
        budget.require(saved['payload']==review.body(packet,review_version,stage,output_tokens=review_output_tokens),'source review request drift')
        text=response_text(evidence/(source_id+'.response.json'))
        reviews.append(dict(stage=stage,raw=text))
        provenance.append(source_id)
        try:
            checked=review.check(f.parse_judgment(text),packet,review_version,stage=stage)
        except (ValueError,KeyError,TypeError):
            stop_stage=stage
            break
        if checked['verdict']!='pass':
            stop_stage=stage
    # Match Rust Plan/ModelConfig serialization field order exactly.
    plan=dict(provider='anthropic-direct-diagnostic',prompt_version=version,variant='audited',
        extractor=dict(model=api.MODEL,timeout_seconds=120,max_output_tokens=2500),
        reviewer=dict(model=api.MODEL,timeout_seconds=120,max_output_tokens=review_output_tokens or (6000 if review_version in {'v35','v36','v37','v38','v39'} else 3000)))
    root=f.ROOT/'prompts'/version
    serialized=json.dumps(plan,ensure_ascii=False,separators=(',',':'))
    material=(f'{version}\n{extraction.prompt(version)}\n'+(root/'review.txt').read_text()+
        '\n'+(root/'frames.txt').read_text()+'\n'+serialized+'\n'+(root/'reader.txt').read_text())
    if version=='extraction-v39':
        material+='\n'+(root/'facets.json').read_text()
    row=dict(case_id=case_id,corpus_case=packet['case'],source_hash=f.sha(packet['case']['source'].encode()),
        raw=raw,variant='audited',plan=plan,prompt_hash=f.sha(material.encode()),reviews=reviews)
    with tempfile.TemporaryDirectory(prefix='prime-radiant-complete-trace-') as temporary:
        src=Path(temporary)/'input.jsonl';dst=Path(temporary)/'output.jsonl'
        src.write_bytes(f.canonical(row)+b'\n')
        result=subprocess.run(['cargo','run','--quiet','--offline','--example','revalidate_extraction',
            '--','--input',str(src),'--output',str(dst)],cwd=f.ROOT,capture_output=True,text=True)
        budget.require(result.returncode==0,'Rust replay failed: '+result.stderr)
        replayed=f.parse_judgment(dst.read_text())
    budget.require(replayed['revalidation']['plan_hash_valid'],'reconstructed Plan hash mismatch')
    receipts=[]
    for ident in provenance:
        usage=read(evidence/(ident+'.accounting.json'))
        actual=api.conservative_usage(read(evidence/(ident+'.response.json')))
        budget.require(usage==actual,'usage drift')
        receipts.append(dict(id=ident,request_sha256=f.sha((evidence/(ident+'.request.json')).read_bytes()),
            response_sha256=f.sha((evidence/(ident+'.response.json')).read_bytes()),
            usage_upper_nanos=usage['conservative_upper_nanos']))
    return dict(status='saved execution replay; diagnostic only',case_id=case_id,
        stopped_at=stop_stage,complete_review_count=len(reviews)==len(candidate['claims'])+len(review.stages(review_version)),
        production_approval=False,independent_semantic_adjudication=False,
        configuration_provenance='reconstructed from exact immutable API requests; no production job Plan',
        revalidation=replayed['revalidation'],claim_count=len(candidate['claims']),
        receipts=receipts,total_trace_upper_nanos=sum(r['usage_upper_nanos'] for r in receipts),
        replay_row=row)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--case',required=True)
    parser.add_argument('--version',choices=['extraction-v29','extraction-v30','extraction-v31','extraction-v32','extraction-v33','extraction-v34','extraction-v35','extraction-v36','extraction-v37','extraction-v38','extraction-v39'],default='extraction-v29')
    parser.add_argument('--review-output-tokens',type=int,choices=[6000])
    parser.add_argument('--candidate-version')
    parser.add_argument('--reader-version')
    parser.add_argument('--evidence',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    result=replay(args.evidence,args.case,args.version,args.candidate_version,args.reader_version,args.review_output_tokens)
    budget.immutable(args.output,result)
    print(json.dumps({k:v for k,v in result.items() if k not in {'receipts','replay_row'}}))

if __name__=='__main__':
    main()
