import unittest
import json
from pathlib import Path
import tempfile

import audit_focused_evidence as audit


class EvidenceAuditTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.result = audit.inspect(audit.EVIDENCE / 'focused-verification/current-clean-revalidated.jsonl')

    def test_exact_composition_corpus_inventories_and_hashes(self):
        result = self.result
        self.assertEqual(result['composition'], {'ko': 8, 'mixed': 2, 'en': 2})
        self.assertEqual(len(result['cases']), 12)
        self.assertEqual(len({c['case_id'] for c in result['cases']}), 12)
        for case in result['cases']:
            self.assertTrue(case['corpus_identity_verified'])
            self.assertEqual(audit.focused.sha(case['original_source'].encode()), case['source_sha256'])
            self.assertEqual(audit.focused.sha(audit.focused.canonical(case['reference_inventory'])),
                             case['reference_sha256'])
            self.assertTrue(case['reference_inventory']['propositions'])
        # An authored selection statement is not independent proof of chronology.
        self.assertIsNone(result['selection_provenance']['chronology_independently_verified'])
        self.assertEqual(result['selection_provenance']['manifest_first_commit'],
                         result['selection_provenance']['packets_first_commit'])

    def test_protocol_counts_editorial_coverage_and_unknown_semantics(self):
        cases = self.result['cases']
        available = [c for c in cases if c['current_profile_available']]
        self.assertEqual(len(available), 7)
        self.assertEqual(sum(c['protocol_status'] == 'eligible' for c in available), 3)
        self.assertEqual(sum(c['protocol_status'] == 'reviewer_rejected' for c in available), 3)
        self.assertEqual(sum(c['protocol_status'] == 'incomplete' for c in available), 1)
        comparison = self.result['before_after']
        self.assertTrue(comparison['measurements_unchanged'])
        for version in ('before', 'after'):
            self.assertEqual(comparison[version]['all']['editorial_valid_protocol_accepted_claims'], 5)
            self.assertEqual(comparison[version]['ko']['editorial_valid_protocol_accepted_claims'], 1)
            self.assertEqual(comparison[version]['all']['available_reference_propositions'], 11)
            self.assertEqual(comparison[version]['ko']['available_reference_propositions'], 7)
            self.assertIsNone(comparison[version]['all']['independently_adjudicated_valid_coverage'])
        self.assertEqual(self.result['live_judgments'], 0)
        self.assertIsNone(self.result['semantic_acceptance'])

    def test_both_historical_error_directions_remain_pinned_and_not_v23(self):
        diagnostics = self.result['historical_error_directions']
        self.assertEqual({d['classification'] for d in diagnostics},
                         {'editorial_false_rejection', 'editorial_false_acceptance'})
        for diagnostic in diagnostics:
            self.assertEqual(diagnostic['plan']['prompt_version'], 'extraction-v3')
            self.assertFalse(diagnostic['current_v23_result'])
            self.assertFalse(diagnostic['independently_adjudicated'])
            self.assertTrue(diagnostic['annotation']['notes'])
        false_reject, false_accept = diagnostics
        self.assertFalse(false_reject['annotation']['accepted'])
        self.assertTrue(false_accept['annotation']['accepted'])
        self.assertEqual(false_reject['source'], '혹시 VPN 비밀번호가 바뀌었나?')
        self.assertEqual(false_accept['candidate']['claims'][1]['text'], 'HTTP adapter는 async다.')

    def test_changed_historical_reviews_cannot_borrow_candidate_identity(self):
        rows = audit.metrics.read_rows(audit.EVIDENCE / 'focused-verification/current-clean-revalidated.jsonl')
        rows[0]['reviews'] = []
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'changed.jsonl'
            path.write_text(''.join(json.dumps(row) + '\n' for row in rows))
            with self.assertRaisesRegex(ValueError, 'archived candidate identity mismatch'):
                audit.inspect(path)
