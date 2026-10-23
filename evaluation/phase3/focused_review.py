"""Offline focused-review preparation. No network, credentials or paid dispatch.

After explicit budget authorization a separate guarded dispatch implementation is
required: current $4.50 ceiling is exhausted and must not be bypassed. Prepared
packets are not judgments and never feed production approval.
"""
import argparse
import collections
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / 'evaluation/phase3/focused-12-manifest.json'
MANIFEST_SHA256 = '164f84834501955b62359552ac192fb6f92fccb44d5a0424a63f8bcdaf8004a7'
PROTOCOL_SHA256 = 'c0c3e603edf7964bbe7f90d4cc73ad09ef5f814525d33199a1b9b8b243173909'
CHECKS = ('atomicity entailment unsupported_assertions negation condition uncertainty '
          'attribution temporal entity_resolution korean_meaning duplicates unnecessary_splitting').split()
STATUSES = {'pass', 'fail', 'unresolved', 'not_applicable'}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()


def load_manifest():
    assert sha(MANIFEST.read_bytes()) == MANIFEST_SHA256, 'frozen manifest changed'
    assert sha((ROOT / 'evaluation/phase3/focused-protocol.txt').read_bytes()) == PROTOCOL_SHA256
    manifest = json.loads(MANIFEST.read_text())
    assert collections.Counter(c['case']['language'] for c in manifest['cases']) == {'ko': 8, 'mixed': 2, 'en': 2}
    for item in manifest['cases']:
        assert sha((ROOT / item['corpus']).read_bytes()) == item['corpus_sha256']
        assert sha(item['case']['source'].encode()) == item['source_sha256']
        assert sha(canonical(item['case']['reference'])) == item['reference_sha256']
    return manifest


def evidence_valid(evidence, source):
    return bool(evidence) and all(
        type(e.get('start')) is int and type(e.get('end')) is int
        and 0 <= e['start'] < e['end'] <= len(source)
        and source[e['start']:e['end']] == e.get('quote') for e in evidence)


def validate_judgment(judgment, packet):
    """Protocol validation only. A well-formed verdict is NOT semantic acceptance."""
    source = packet['case']['source']
    claims = packet['candidate']['claims']
    assert judgment['verdict'] in {'pass', 'fail', 'unresolved'}
    assert isinstance(judgment['uncertainty_flags'], list)
    assert [c['index'] for c in judgment['claims']] == list(range(len(claims)))
    statuses = []
    for checked, claim in zip(judgment['claims'], claims):
        assert checked['text'] == claim['text']
        assert evidence_valid(checked['evidence'], source)
        assert set(checked['checks']) == set(CHECKS)
        assert isinstance(checked['uncertainty_flags'], list)
        for check in checked['checks'].values():
            assert check['status'] in STATUSES and check['reason'].strip()
            statuses.append(check['status'])
    references = packet['case']['reference']['propositions']
    assert [r['reference_index'] for r in judgment['inventory']] == list(range(len(references)))
    for reference in judgment['inventory']:
        assert reference['status'] in STATUSES and reference['reason'].strip()
        assert evidence_valid(reference['evidence'], source)
        assert all(type(i) is int and 0 <= i < len(claims) for i in reference['claim_indices'])
        assert reference['status'] != 'pass' or reference['claim_indices']
        statuses.append(reference['status'])
    if judgment['verdict'] == 'pass':
        assert not {'fail', 'unresolved'} & set(statuses)
        assert not judgment['uncertainty_flags']
        assert not any(c['uncertainty_flags'] for c in judgment['claims'])
    return True


def prepare(output, binary):
    manifest = load_manifest()
    selected = {c['case']['id']: c for c in manifest['cases']}
    output.mkdir()  # Immutable runs; never overwrite evidence.
    rows = []
    for path in sorted((ROOT / 'reports/evidence/phase-3').glob('*.jsonl.gz')):
        artifact_hash = sha(path.read_bytes())
        with gzip.open(path, 'rt') as stream:
            for line_number, line in enumerate(stream, 1):
                row = json.loads(line)
                item = selected.get(row.get('case_id'))
                if not item or not all(k in row for k in ('corpus_case', 'plan', 'source_hash', 'prompt_hash')):
                    continue
                if row['source_hash'] != item['source_sha256'] or row['corpus_case']['source'] != item['case']['source']:
                    continue
                if row['corpus_case'].get('authored_at') != item['case'].get('authored_at'):
                    continue
                row['archive_provenance'] = dict(path=str(path.relative_to(ROOT)), sha256=artifact_hash, line=line_number)
                rows.append(row)
    with tempfile.TemporaryDirectory() as temporary:
        source, target = (Path(temporary) / x for x in ('input', 'output'))
        source.write_text(''.join(json.dumps(r, ensure_ascii=False) + '\n' for r in rows))
        subprocess.run([str(binary.resolve()), '--input', str(source), '--output', str(target)], check=True)
        replay = [json.loads(line) for line in target.read_text().splitlines()]
    (output / 'replay.jsonl.gz').write_bytes(gzip.compress(b''.join(canonical(r) + b'\n' for r in replay), mtime=0))
    packets, gaps = [], []
    for case_id, item in selected.items():
        eligible = [r for r in replay if r['case_id'] == case_id
                    and r['plan'].get('prompt_version') == 'extraction-v23'
                    and r['variant'] == 'audited' and r.get('raw')
                    and r['revalidation']['plan_hash_valid']]
        eligible.sort(key=lambda r: ('initial' not in r['archive_provenance']['path'],
                                    r['archive_provenance']['path'], r['run'], r.get('attempt', 1),
                                    r['archive_provenance']['line']))
        if not eligible:
            gaps.append(case_id)
            continue
        row = eligible[0]
        try:
            candidate = json.loads(row['raw'])
            assert isinstance(candidate['claims'], list)
        except (ValueError, KeyError, TypeError, AssertionError):
            gaps.append(case_id)  # Do not select a later successful repair instead.
            continue
        packet = dict(case=item['case'], source_sha256=item['source_sha256'],
                      reference_sha256=item['reference_sha256'], candidate=candidate,
                      candidate_sha256=sha(row['raw'].encode()), plan=row['plan'],
                      prompt_hash=row['prompt_hash'], provenance=row['archive_provenance'],
                      structural_replay=row['revalidation'])
        packets.append(packet)
    (output / 'packets.json').write_text(json.dumps(packets, ensure_ascii=False, indent=2) + '\n')
    summary = dict(status='Blocked', manifest_sha256=MANIFEST_SHA256,
                   protocol_sha256=sha((ROOT / 'evaluation/phase3/focused-protocol.txt').read_bytes()),
                   selected_cases=12, prepared_primary=len(packets), coverage_gaps=gaps,
                   prepared_repeats=[p['case']['id'] for p in packets if p['case']['id'] in manifest['repeated_cases']],
                   archived_rows=len(replay), valid_new_model_judgments=0,
                   accepted_batches=0, rejected_batches=0, semantic_valid_claim_coverage=None,
                   korean_semantic_valid_claim_coverage=None, new_paid_requests=0, new_spend_usd=0,
                   blocker='Existing ledger prior $4.78265335 exceeds $4.50 ceiling; no authorization to change it.')
    (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/examples/revalidate_extraction')
    args = parser.parse_args()
    prepare(args.output, args.binary)
