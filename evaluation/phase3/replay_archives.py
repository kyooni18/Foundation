"""Replay archived extraction payloads offline; never infer semantic correctness.

Build `cargo build --offline --example revalidate_extraction` first. This script
calls only that local revalidator (temporary SQLite database, no model adapter).
Original evidence is read-only. Outputs must go to a new directory.
"""
import argparse
import collections
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

IDENTITY = ['case_id', 'source_hash', 'plan', 'prompt_hash', 'variant', 'run',
            'attempt', 'raw', 'reviews']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', type=Path, default=Path('reports/evidence/phase-3'))
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=Path('target/debug/examples/revalidate_extraction'))
    args = parser.parse_args()
    args.output.mkdir()
    archives = {}
    with tempfile.TemporaryDirectory(prefix='primeradiant-replay-') as temporary:
        input_path, result_path = (Path(temporary) / name for name in ['input.jsonl', 'result.jsonl'])
        with input_path.open('w') as combined:
            for path in sorted(args.evidence.glob('*.jsonl.gz')):
                included = 0
                with gzip.open(path, 'rt') as source:
                    for line in source:
                        row = json.loads(line)
                        if not all(key in row for key in ['corpus_case', 'plan', 'source_hash']):
                            continue
                        row['replay_artifact'] = str(path)
                        combined.write(json.dumps(row, ensure_ascii=False) + '\n')
                        included += 1
                if included:
                    archives[str(path)] = dict(rows=included, sha256=hashlib.sha256(path.read_bytes()).hexdigest())
        subprocess.run([str(args.binary.resolve()), '--input', str(input_path), '--output', str(result_path)], check=True)
        index, seen, sources = [], set(), set()
        profiles = collections.defaultdict(collections.Counter)
        errors = collections.Counter()
        changes = 0
        for line in result_path.read_text().splitlines():
            row = json.loads(line)
            identity = json.dumps([row.get(key) for key in IDENTITY], sort_keys=True, ensure_ascii=False)
            digest = hashlib.sha256(identity.encode()).hexdigest()
            if digest in seen:
                continue
            seen.add(digest)
            sources.add(row['source_hash'])
            profile = row['plan'].get('prompt_version') or 'extraction-v1'
            check = row['revalidation']
            profiles[profile].update(attempts=1, structurally_valid=int(check['valid']),
                                     plan_hash_valid=int(check['plan_hash_valid']),
                                     experimental_gate_eligible=int(check['experimental_gate_eligible']))
            errors[str(check['error'])] += 1
            changes += row.get('structural_valid') is True and not check['valid']
            index.append(dict(payload_sha256=digest, artifact=row['replay_artifact'],
                              case_id=row['case_id'], run=row['run'], attempt=row.get('attempt', 1),
                              source_hash=row['source_hash'], prompt_hash=row['prompt_hash'], profile=profile,
                              original_structural_valid=row.get('structural_valid'), revalidation=check))
    summary = dict(artifact_rows=sum(a['rows'] for a in archives.values()), unique_attempt_payloads=len(seen),
                   unique_sources=len(sources), errors=dict(errors),
                   historical_structural_pass_now_rejected=changes, semantic_acceptance=False)
    for key in ['plan_hash_valid', 'structurally_valid', 'experimental_gate_eligible']:
        summary[key] = sum(counts[key] for counts in profiles.values())
    for name, data in [('replay-summary', summary), ('replay-by-profile', profiles), ('input-manifest', archives)]:
        (args.output / (name + '.json')).write_text(json.dumps(data, indent=2) + '\n')
    payload = ''.join(json.dumps(row, ensure_ascii=False) + '\n' for row in index).encode()
    (args.output / 'replay-index.jsonl.gz').write_bytes(gzip.compress(payload, mtime=0))
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
