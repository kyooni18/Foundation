"""Aggregate adjudicated semantic labels; missing labels never count as passes.
Usage: python3 score.py run.jsonl > metrics.json
Each row.human_review must contain reviewer, accepted (bool), supported_propositions,
covered_propositions, claims (list of per-claim bool flags below), oversplits (int),
decompositions (int), translation_errors (int), translation_candidates (int).
Independent human adjudication is required for acceptance; model reviews are diagnostic.
"""
import collections,json,sys
rows=[json.loads(line) for line in open(sys.argv[1])]
flags=['atomicity_violation','unsupported','fidelity_error','epistemic_error','language_error','anchor_error','duplicate']
groups=collections.defaultdict(list)
for row in rows:
    for key in [(row['variant'],row['language'],'all'),(row['variant'],row['language'],row['category'])]:
        groups[key].append(row)
output={}
for key,rs in groups.items():
    reviewed=[r for r in rs if r.get('human_review') is not None]
    for r in reviewed:
        h=r['human_review']
        assert h.get('reviewer') and type(h['accepted']) is bool
        assert all(type(c[f]) is bool for c in h['claims'] for f in flags)
        assert 0<=h['covered_propositions']<=h['supported_propositions']
    accepted=[r for r in reviewed if r['human_review']['accepted']]
    claims=[c for r in accepted for c in r['human_review']['claims']]
    rate=lambda n,d: {'numerator':n,'denominator':d,'rate':n/d if d else None}
    item={'runs':len(rs),'reviewed':len(reviewed),'accepted_batches':len(accepted),'unreviewed':len(rs)-len(reviewed)}
    for f in flags:item[f]=rate(sum(c[f] for c in claims),len(claims))
    # End-to-end coverage includes rejected batches as zero, rather than hiding failures.
    item['coverage']=rate(sum(r['human_review']['covered_propositions'] if r['human_review']['accepted'] else 0 for r in reviewed),sum(r['human_review']['supported_propositions'] for r in reviewed))
    item['oversplit']=rate(sum(r['human_review']['oversplits'] for r in accepted),sum(r['human_review']['decompositions'] for r in accepted))
    item['translation_errors']=rate(sum(r['human_review']['translation_errors'] for r in reviewed),sum(r['human_review']['translation_candidates'] for r in reviewed))
    # Lexical repeatability is diagnostic only; human semantic comparison remains required.
    per_case=collections.defaultdict(set)
    for r in rs:
        if 'raw' in r: per_case[r['case_id']].add(r['raw'])
    item['distinct_outputs_per_case']={k:len(v) for k,v in per_case.items()}
    confusion=collections.Counter()
    for r in reviewed:
        bad=any(any(c[f] for f in flags) for c in r['human_review']['claims']) or r['human_review']['covered_propositions']<r['human_review']['supported_propositions']
        for review in r.get('reviews',[]):
            try: rejects=json.loads(review['raw'])['verdict']!='pass'
            except (KeyError,ValueError): continue
            confusion[f"{review['stage']}:{'TP' if bad and rejects else 'FN' if bad else 'FP' if rejects else 'TN'}"]+=1
    item['reviewer_confusion']=dict(confusion)
    output['|'.join(key)]=item
print(json.dumps(output,ensure_ascii=False,indent=2))
