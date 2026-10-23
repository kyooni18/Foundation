import copy
import json
import unittest

import focused_metrics as metrics
import focused_review as focused


class SemanticReplayTest(unittest.TestCase):
    def setUp(self):
        evidence = focused.ROOT / 'reports/evidence/phase-3/focused-continuation'
        self.packets = json.loads((evidence / 'replay/packets.json').read_text())
        self.annotations = {r['case_id']: r for r in metrics.read_rows(evidence / 'editorial-inspection.jsonl')}
        self.cases = {r['case']['id']: r for r in focused.load_manifest()['cases']}
        self.validations = {p['case']['id']: p['structural_replay'] for p in self.packets}

    def test_frozen_semantic_regression_counts_preserve_unknowns_and_korean_breakdown(self):
        all_cases = metrics.measure(self.cases, self.packets, self.annotations, self.validations)
        self.assertEqual((all_cases['protocol_accepted_batches'], all_cases['protocol_rejected_or_incomplete_batches']), (3, 4))
        self.assertEqual((all_cases['editorial_pass_batches'], all_cases['editorial_unresolved_batches']), (5, 2))
        self.assertEqual(all_cases['covered_references_in_protocol_accepted_editorially_valid_batches'], 5)
        self.assertEqual(all_cases['available_reference_propositions'], 11)
        self.assertEqual(all_cases['missing_sources'], 5)
        self.assertIsNone(all_cases['full_selected_set_semantic_coverage'])
        self.assertIsNone(all_cases['independently_adjudicated_valid_coverage'])
        korean = {k: v for k, v in self.cases.items() if v['case']['language'] == 'ko'}
        result = metrics.measure(korean, self.packets, self.annotations, self.validations)
        self.assertEqual((result['protocol_accepted_batches'], result['protocol_rejected_or_incomplete_batches']), (1, 4))
        self.assertEqual(result['covered_references_in_protocol_accepted_editorially_valid_batches'], 1)
        self.assertEqual(result['available_reference_propositions'], 7)
        self.assertEqual(result['missing_sources'], 3)

    def test_rejected_batches_and_contested_claims_never_inflate_valid_coverage(self):
        validations = copy.deepcopy(self.validations)
        for value in validations.values():
            value['experimental_gate_eligible'] = False
        result = metrics.measure(self.cases, self.packets, self.annotations, validations)
        self.assertEqual(result['editorial_valid_protocol_accepted_claims'], 0)
        self.assertEqual(result['covered_references_in_protocol_accepted_editorially_valid_batches'], 0)
        # Even an experimental gate pass cannot resolve the contested atomicity.
        validations['r1-ambiguous_ref']['experimental_gate_eligible'] = True
        result = metrics.measure(self.cases, self.packets, self.annotations, validations)
        self.assertEqual(result['protocol_accepted_claims'], 2)
        self.assertEqual(result['editorial_valid_protocol_accepted_claims'], 0)
        self.assertEqual(result['covered_references_in_protocol_accepted_editorially_valid_batches'], 0)
