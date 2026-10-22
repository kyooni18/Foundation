"""Aggregate adjudicated semantic labels; missing labels never count as passes.
Usage: python3 score.py run.jsonl > metrics.json
Each row.human_review must contain reviewer, accepted (bool), supported_propositions,
covered_propositions, claims (list of per-claim bool flags below), oversplits (int),
decompositions (int), translation_errors (int), translation_candidates (int).
Independent human adjudication is required for acceptance; model reviews are diagnostic.
"""
import argparse,collections,json,sys
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('input')
parser.add_argument('--annotation-field', choices=['human_review','assistant_review'], default='human_review')
args=parser.parse_args()
annotation=args.annotation_field
flags=['atomicity_violation','unsupported','fidelity_error','epistemic_error','language_error','anchor_error','duplicate']

def require(condition, message):
    if not condition:
        raise ValueError(message)

def validate_review(row):
    h = row.get(annotation)
    if h is None:
        return
    require(isinstance(h, dict), 'human_review must be an object')
    require(isinstance(h.get('reviewer'), str) and h['reviewer'].strip(), 'reviewer required')
    require(type(h.get('accepted')) is bool, 'accepted must be boolean')
    for key in ['supported_propositions', 'covered_propositions', 'oversplits',
                'decompositions', 'translation_errors', 'translation_candidates']:
        require(type(h.get(key)) is int and h[key] >= 0, f'{key} must be a nonnegative integer')
    for numerator, denominator in [('covered_propositions', 'supported_propositions'),
                                   ('oversplits', 'decompositions'),
                                   ('translation_errors', 'translation_candidates')]:
        require(h[numerator] <= h[denominator], f'{numerator} exceeds denominator')
    require(isinstance(h.get('claims'), list), 'per-claim annotations required')
    try:
        original_claims = json.loads(row.get('raw', 'null'))['claims']
    except (ValueError, KeyError, TypeError):
        original_claims = []
    require(isinstance(original_claims, list), 'raw claims must be an array')
    require(len(h['claims']) == len(original_claims), 'annotate every generated Claim exactly once, including rejected output')
    for claim in h['claims']:
        require(isinstance(claim, dict) and all(type(claim.get(f)) is bool for f in flags),
                'every Claim needs all error flags as booleans')
    if h['accepted']:
        require(row.get('structural_valid') is True, 'structurally invalid output cannot be accepted')
    if not h['claims']:
        require(h['covered_propositions'] == 0, 'empty output cannot cover propositions')

rows=[json.loads(line) for line in open(args.input)]
seen=set()
for row in rows:
    identity=(row['case_id'],row['variant'],row['run'],row['prompt_hash'],row['source_hash'])
    require(identity not in seen, 'duplicate evaluation run')
    seen.add(identity)
    validate_review(row)
groups=collections.defaultdict(list)
for row in rows:
    plan=row.get('plan',{})
    context=(plan.get('prompt_version') or 'extraction-v1',plan.get('extractor',{}).get('model','unspecified'),plan.get('reviewer',{}).get('model','unspecified'))
    for key in [(row['variant'],row['language'],'all'),(row['variant'],row['language'],row['category'])]:
        groups[context+key].append(row)
output={}
for key,rs in groups.items():
    reviewed=[r for r in rs if r.get(annotation) is not None]
    accepted=[r for r in reviewed if r[annotation]['accepted']]
    claims=[c for r in accepted for c in r[annotation]['claims']]
    generated=[c for r in reviewed for c in r[annotation]['claims']]
    rate=lambda n,d: {'numerator':n,'denominator':d,'rate':n/d if d else None}
    item={'annotation_source':annotation,'runs':len(rs),'reviewed':len(reviewed),'accepted_batches':len(accepted),'unreviewed':len(rs)-len(reviewed)}
    for f in flags:
        item[f]=rate(sum(c[f] for c in claims),len(claims))
        item['generated_'+f]=rate(sum(c[f] for c in generated),len(generated))
    # End-to-end coverage includes rejected batches as zero, rather than hiding failures.
    item['coverage']=rate(sum(r[annotation]['covered_propositions'] if r[annotation]['accepted'] else 0 for r in reviewed),sum(r[annotation]['supported_propositions'] for r in reviewed))
    item['generated_coverage']=rate(sum(r[annotation]['covered_propositions'] for r in reviewed),sum(r[annotation]['supported_propositions'] for r in reviewed))
    item['oversplit']=rate(sum(r[annotation]['oversplits'] for r in accepted),sum(r[annotation]['decompositions'] for r in accepted))
    item['translation_errors']=rate(sum(r[annotation]['translation_errors'] for r in reviewed),sum(r[annotation]['translation_candidates'] for r in reviewed))
    # Lexical repeatability is diagnostic only; human semantic comparison remains required.
    per_case=collections.defaultdict(set)
    for r in rs:
        if 'raw' in r: per_case[r['case_id']].add(r['raw'])
    item['distinct_outputs_per_case']={k:len(v) for k,v in per_case.items()}
    confusion=collections.Counter()
    for r in reviewed:
        bad=r[annotation]['oversplits']>0 or any(any(c[f] for f in flags) for c in r[annotation]['claims']) or r[annotation]['covered_propositions']<r[annotation]['supported_propositions']
        for review in r.get('reviews',[]):
            try: rejects=json.loads(review['raw'])['verdict']!='pass'
            except (KeyError,ValueError): continue
            confusion[f"{review['stage']}:{'TP' if bad and rejects else 'FN' if bad else 'FP' if rejects else 'TN'}"]+=1
    item['reviewer_confusion']=dict(confusion)
    output['|'.join(key)]=item
print(json.dumps(output,ensure_ascii=False,indent=2))
