"""Offline focused-review preparation. No network, credentials or paid dispatch.

The guarded_haiku dispatcher requires explicit authorization and budget headroom;
the authorized ceiling still requires a compatible provider cap. Prepared
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


def has_korean(text):
    """Mirror the core's conservative Hangul/Jamo presence guard, without normalization."""
    return any(0x1100 <= ord(c) <= 0x11ff or 0x3130 <= ord(c) <= 0x318f or
               0xac00 <= ord(c) <= 0xd7af for c in text)


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


def parse_judgment(raw):
    """Conflicting JSON members must not be collapsed into a convenient verdict."""
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError('duplicate judgment member')
            result[key] = value
        return result
    def invalid_constant(value):
        raise ValueError('nonfinite judgment value')
    return json.loads(raw, object_pairs_hook=unique, parse_constant=invalid_constant)


def validate_judgment(judgment, packet):
    """Protocol validation only. A well-formed verdict is NOT semantic acceptance."""
    source = packet['case']['source']
    claims = packet['candidate']['claims']
    assert judgment['verdict'] in {'pass', 'fail', 'unresolved'}
    assert isinstance(judgment['uncertainty_flags'], list)
    assert all(type(c['index']) is int for c in judgment['claims'])
    assert [c['index'] for c in judgment['claims']] == list(range(len(claims)))
    statuses = []
    for checked, claim in zip(judgment['claims'], claims):
        assert checked['text'] == claim['text']
        assert evidence_valid(checked['evidence'], source)
        assert set(checked['checks']) == set(CHECKS)
        # Every nonempty Claim has atomicity and entailment obligations. N/A is
        # not a way to turn an unmeasured core requirement into a passing verdict.
        for required in ('atomicity', 'entailment', 'unsupported_assertions'):
            assert checked['checks'][required]['status'] != 'not_applicable'
        if packet['case'].get('language') in {'ko', 'mixed'} or claim.get('lang') == 'ko':
            assert checked['checks']['korean_meaning']['status'] != 'not_applicable'
        assert isinstance(checked['uncertainty_flags'], list)
        for check in checked['checks'].values():
            assert check['status'] in STATUSES and check['reason'].strip()
            statuses.append(check['status'])
    references = packet['case']['reference']['propositions']
    assert all(type(r['reference_index']) is int for r in judgment['inventory'])
    assert [r['reference_index'] for r in judgment['inventory']] == list(range(len(references)))
    for reference in judgment['inventory']:
        # Every frozen inventory proposition needs a coverage judgment. A disputed
        # reference is unresolved, never N/A to manufacture a passing batch.
        assert reference['status'] in {'pass', 'fail', 'unresolved'} and reference['reason'].strip()
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
                   blocker='Offline preparation supplies no judgments; authorized dispatch requires budget headroom and a compatible non-resetting provider cap.')
    (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/examples/revalidate_extraction')
    args = parser.parse_args()
    prepare(args.output, args.binary)
