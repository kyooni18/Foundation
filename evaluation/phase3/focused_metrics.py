"""Reproducible offline before/after measurements; never semantic acceptance.

The same frozen assistant editorial annotations are applied retrospectively to
identical archived outputs. This measures regression, not reviewer repair efficacy.
"""
import argparse
import json
from pathlib import Path

import focused_review as focused


def read_rows(path):
    return [json.loads(line) for line in path.read_text().splitlines()]


def measure(cases, packets, annotations, validations):
    available = [p for p in packets if p['case']['id'] in cases]
    result = dict(selected_sources=len(cases), available_sources=len(available),
                  missing_sources=len(cases) - len(available), generated_claims=0,
                  protocol_accepted_batches=0, protocol_rejected_or_incomplete_batches=0,
                  editorial_pass_batches=0, editorial_fail_batches=0,
                  editorial_unresolved_batches=0, available_reference_propositions=0,
                  protocol_accepted_claims=0, editorial_valid_protocol_accepted_claims=0,
                  covered_references_in_protocol_accepted_editorially_valid_batches=0,
                  production_published_claims=0, independently_adjudicated_valid_coverage=None,
                  full_selected_set_semantic_coverage=None)
    for packet in available:
        case_id = packet['case']['id']
        judgment = annotations[case_id]['judgment']
        focused.validate_judgment(judgment, packet)
        count = len(packet['candidate']['claims'])
        result['generated_claims'] += count
        result['available_reference_propositions'] += len(packet['case']['reference']['propositions'])
        result['editorial_' + judgment['verdict'] + '_batches'] += 1
        accepted = validations[case_id]['experimental_gate_eligible']
        result['protocol_accepted_batches' if accepted else 'protocol_rejected_or_incomplete_batches'] += 1
        if not accepted:
            continue  # Rejected/incomplete batches contribute NO covered propositions.
        result['protocol_accepted_claims'] += count
        valid_indices = {c['index'] for c in judgment['claims']
                         if not c['uncertainty_flags']
                         and all(v['status'] in {'pass', 'not_applicable'} for v in c['checks'].values())}
        result['editorial_valid_protocol_accepted_claims'] += len(valid_indices)
        if judgment['verdict'] == 'pass':
            result['covered_references_in_protocol_accepted_editorially_valid_batches'] += sum(
                r['status'] == 'pass' and bool(r['claim_indices'])
                and set(r['claim_indices']) <= valid_indices for r in judgment['inventory'])
    return result


def comparison(baseline, current, packet_path, annotation_path):
    manifest = focused.load_manifest()
    packets = json.loads(packet_path.read_text())
    annotations = {r['case_id']: r for r in read_rows(annotation_path)}
    cases = {r['case']['id']: r for r in manifest['cases']}
    views = []
    for path in (baseline, current):
        rows = read_rows(path)
        assert len(rows) == len(packets)
        checks = {}
        for packet in packets:
            case_id = packet['case']['id']
            matching = [r for r in rows if r['case_id'] == case_id
                        and focused.sha(r['raw'].encode()) == packet['candidate_sha256']
                        and r['source_hash'] == packet['source_sha256']
                        and r['prompt_hash'] == packet['prompt_hash']
                        and r['plan'] == packet['plan']]
            assert len(matching) == 1, 'before/after identity mismatch'
            assert matching[0]['revalidation']['plan_hash_valid']
            assert annotations[case_id]['candidate_sha256'] == packet['candidate_sha256']
            assert annotations[case_id]['source_sha256'] == packet['source_sha256']
            checks[case_id] = matching[0]['revalidation']
        views.append({language: measure(
            {key: value for key, value in cases.items()
             if language == 'all' or value['case']['language'] == language},
            packets, annotations, checks) for language in ('all', 'ko', 'mixed', 'en')})
    return dict(baseline_commit='5be9c669ddae79686346a12752afaa0887374c62',
                measurement='offline replay plus retrospective assistant editorial labels; not live model judgments',
                inputs={str(p): focused.sha(p.read_bytes()) for p in (baseline, current, packet_path, annotation_path)},
                before=views[0], after=views[1], measurements_unchanged=views[0] == views[1],
                reviewer_repair_effectiveness=None, new_model_judgments=0)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--current', type=Path, required=True)
    parser.add_argument('--packets', type=Path, default=focused.ROOT / 'reports/evidence/phase-3/focused-continuation/replay/packets.json')
    parser.add_argument('--annotations', type=Path, default=focused.ROOT / 'reports/evidence/phase-3/focused-continuation/editorial-inspection.jsonl')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = comparison(args.baseline, args.current, args.packets, args.annotations)
    with args.output.open('x') as output:
        output.write(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
