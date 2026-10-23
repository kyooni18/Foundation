"""Inspect frozen source inventories, archived judgments and operational budget offline.

No network or credential access. Writes only to a new output directory. Does not
claim independent chronology, editorial gold, live judgments or semantic acceptance.
"""
import argparse
from collections import Counter
import gzip
import json
from pathlib import Path
import sqlite3
import subprocess

import focused_metrics as metrics
import focused_review as focused
import guarded_haiku as guarded

EVIDENCE = focused.ROOT / 'reports/evidence/phase-3'


def git_first(path):
    return subprocess.check_output(['git', 'log', '--reverse', '--format=%H', '--',
                                    str(path.relative_to(focused.ROOT))], cwd=focused.ROOT,
                                   text=True).splitlines()[0]


def inspect(current):
    manifest, packets = guarded.packets()
    by_id = {p['case']['id']: p for p in packets}
    annotations = {r['case_id']: r for r in metrics.read_rows(EVIDENCE / 'focused-continuation/editorial-inspection.jsonl')}
    rows = {r['case_id']: r for r in metrics.read_rows(current)}
    inventories = []
    for item in manifest['cases']:
        case = item['case']
        corpus = json.loads((focused.ROOT / item['corpus']).read_text())
        guarded.require([c for c in corpus if c['id'] == case['id']] == [case], 'corpus inventory mismatch')
        entry = dict(case_id=case['id'], language=case['language'], category=case.get('category'),
                     original_source=case['source'], reference_inventory=case['reference'],
                     source_sha256=item['source_sha256'], reference_sha256=item['reference_sha256'],
                     corpus=item['corpus'], corpus_sha256=item['corpus_sha256'],
                     corpus_identity_verified=True, current_profile_available=case['id'] in by_id)
        if case['id'] in by_id:
            packet, row = by_id[case['id']], rows[case['id']]
            provenance = packet['provenance']
            archive = focused.ROOT / provenance['path']
            with gzip.open(archive, 'rt') as stream:
                archived = json.loads(next(line for n, line in enumerate(stream, 1) if n == provenance['line']))
            for key in ('raw', 'plan', 'prompt_hash', 'source_hash', 'reviews', 'case_id', 'run', 'attempt'):
                guarded.require(archived.get(key) == row.get(key), 'archived candidate identity mismatch')
            guarded.require(archived['corpus_case']['source'] == case['source'], 'original source mismatch')
            judgment = annotations[case['id']]['judgment']
            focused.validate_judgment(judgment, packet)
            eligible = row['revalidation']['experimental_gate_eligible']
            rejected = any(json.loads(r['raw'])['verdict'] == 'reject' for r in row['reviews'] if r.get('raw'))
            errors = [r['error'] for r in row['reviews'] if r.get('error')]
            entry.update(candidate=packet['candidate'], archive_provenance=provenance,
                plan=packet['plan'], prompt_hash=packet['prompt_hash'], candidate_sha256=packet['candidate_sha256'],
                protocol_status='eligible' if eligible else ('reviewer_rejected' if rejected else 'incomplete'),
                recorded_failure=row.get('failure_code'), additional_provider_errors=errors,
                editorial_judgment=judgment, editorial_authority='assistant only; not independent human gold',
                semantic_acceptance=None, production_published_claims=0)
        inventories.append(entry)
    comparisons = metrics.comparison(
        EVIDENCE / 'focused-verification/baseline-revalidated.jsonl', current,
        guarded.PACKETS, EVIDENCE / 'focused-continuation/editorial-inspection.jsonl')
    # Preserve BOTH error directions with original historical model/profile identities.
    audit_path = EVIDENCE / 'luna-v3-assistant-audit.jsonl'
    historical = metrics.read_rows(audit_path)
    archive_path = EVIDENCE / 'luna-dev-v3.jsonl.gz'
    with gzip.open(archive_path, 'rt') as stream:
        archived = [json.loads(line) for line in stream]
    diagnostic = []
    for audit_line, archive_line, kind in [(13, 13, 'editorial_false_rejection'), (49, 145, 'editorial_false_acceptance')]:
        row, original = historical[audit_line - 1], archived[archive_line - 1]
        for key in ('raw', 'source_hash', 'prompt_hash', 'plan', 'reviews', 'case_id', 'run'):
            guarded.require(row[key] == original[key], 'historical diagnostic mismatch')
        guarded.require(focused.sha(row['corpus_case']['source'].encode()) == row['source_hash'], 'historical source hash')
        diagnostic.append(dict(classification=kind, source=row['corpus_case']['source'],
            source_sha256=row['source_hash'], plan=row['plan'], prompt_hash=row['prompt_hash'],
            candidate=json.loads(row['raw']), reviews=row['reviews'], annotation=row['assistant_review'],
            audit=dict(path=str(audit_path.relative_to(focused.ROOT)), line=audit_line, sha256=focused.sha(audit_path.read_bytes())),
            archive=dict(path=str(archive_path.relative_to(focused.ROOT)), line=archive_line, sha256=focused.sha(archive_path.read_bytes())),
            current_v23_result=False, independently_adjudicated=False))
    first_manifest, first_packets = git_first(focused.MANIFEST), git_first(guarded.PACKETS)
    return dict(composition=dict(Counter(i['language'] for i in inventories)),
        manifest_sha256=focused.sha(focused.MANIFEST.read_bytes()), protocol_sha256=focused.PROTOCOL_SHA256,
        cases=inventories, before_after=comparisons, historical_error_directions=diagnostic,
        selection_provenance=dict(manifest_first_commit=first_manifest, packets_first_commit=first_packets,
            recorded_selection_rule=manifest['selection'], outcome_independent_selection_rule=manifest['primary_selection'],
            chronology_independently_verified=None,
            limitation='Manifest and focused packets entered the same commit. Git proves frozen contents, not within-session order of inspection. Sources and historical responses were already exposed. No final live outcomes exist; do not describe this as prospectively blinded gold.'),
        semantic_acceptance=None, live_judgments=0, repeated_live_judgments=0)


def budget_audit(path):
    before = focused.sha(path.read_bytes())
    conn = sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True)
    queries = ['SELECT ceiling,prior FROM budget WHERE id=1',
               'SELECT COUNT(*), COALESCE(SUM(actual),0), COALESCE(SUM(CASE WHEN actual IS NULL THEN estimate ELSE 0 END),0) FROM request']
    try:
        conn.execute('PRAGMA query_only=ON')
        conn.execute('BEGIN')
        results = [list(conn.execute(query).fetchone()) for query in queries]
    finally:
        conn.close()
    after = focused.sha(path.read_bytes())
    guarded.require(before == after, 'ledger changed during audit')
    ceiling, prior = results[0]
    _, spent, pending = results[1]
    return dict(queries=queries, rows=results, amounts='integer nano-USD',
                before_sha256=before, after_sha256=after,
                authorized_headroom_nanos=max(0, min(ceiling, guarded.LIMIT) - prior - spent - pending),
                new_spending_authorization='not assessed by read-only accounting audit; consult the applicable user authorization record',
                dispatch_permitted=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--current', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = inspect(args.current)
    budget = budget_audit(guarded.LEDGER)
    args.output.mkdir()
    guarded.immutable(args.output / 'selected-evidence-audit.json', result)
    guarded.immutable(args.output / 'budget-readonly.json', budget)
    print(json.dumps({'composition': result['composition'],
                      'measurements_unchanged': result['before_after']['measurements_unchanged'],
                      'headroom_nanos': budget['authorized_headroom_nanos'],
                      'semantic_acceptance': None}, indent=2))
