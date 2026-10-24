"""Read-only replay of saved direct Claude judgments; no semantic approval or network."""
import hashlib
import json
from pathlib import Path

import focused_review as focused

ROOT = focused.ROOT
EVIDENCE = ROOT / 'reports/evidence/phase-3/direct-anthropic/o8000'


def replay():
    rows = []
    for response_path in sorted(EVIDENCE.glob('*.response.json')):
        stem = response_path.name.removesuffix('.response.json')
        request_path = EVIDENCE / (stem + '.request.json')
        response = json.loads(response_path.read_text())
        raw = ''.join(block.get('text', '') for block in response.get('content', [])
                      if block.get('type') == 'text')
        judgment = focused.parse_judgment(raw)
        statuses = [dimension['status'] for claim in judgment['claims']
                    for dimension in claim['checks'].values()]
        statuses += [item['status'] for item in judgment['inventory']]
        flags = judgment['uncertainty_flags'] or any(c['uncertainty_flags'] for c in judgment['claims'])
        expected = ('fail' if 'fail' in statuses else
                    'unresolved' if 'unresolved' in statuses or flags else 'pass')
        row = dict(id=stem, request_sha256=hashlib.sha256(request_path.read_bytes()).hexdigest(),
                   response_sha256=hashlib.sha256(response_path.read_bytes()).hexdigest(),
                   model_verdict=judgment['verdict'], dimension_consistent_verdict=expected,
                   aggregate_consistent=judgment['verdict'] == expected,
                   korean_not_applicable_claims=[c['index'] for c in judgment['claims']
                       if c['checks']['korean_meaning']['status'] == 'not_applicable'],
                   claim_count=len(judgment['claims']), inventory_count=len(judgment['inventory']))
        rows.append(row)
    return dict(evidence='historical v23/o8000 unchanged; retrospective protocol consistency only',
                responses=len(rows), aggregate_consistent=sum(r['aggregate_consistent'] for r in rows),
                aggregate_inconsistent=sum(not r['aggregate_consistent'] for r in rows),
                semantic_quality_after=None, independently_adjudicated_ground_truth=False,
                rows=rows)


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    with args.output.open('x') as f:
        json.dump(replay(), f, ensure_ascii=False, indent=2)
        f.write('\n')
