import copy
import unittest

import focused_review as focused


class FocusedTest(unittest.TestCase):
    def fixture(self):
        packet = {'case': {'source': '한글 source', 'reference': {'propositions': ['one']}},
                  'candidate': {'claims': [{'text': 'one'}]}}
        evidence = [{'start': 0, 'end': 2, 'quote': '한글'}]
        judgment = {'claims': [{'index': 0, 'text': 'one', 'evidence': evidence,
                               'checks': {k: {'status': 'pass', 'reason': 'fixture only'} for k in focused.CHECKS},
                               'uncertainty_flags': []}],
                    'inventory': [{'reference_index': 0, 'status': 'pass', 'claim_indices': [0],
                                   'evidence': evidence, 'reason': 'fixture only'}],
                    'verdict': 'pass', 'uncertainty_flags': []}
        return packet, judgment

    def test_manifest_is_frozen_and_balanced(self):
        manifest = focused.load_manifest()
        self.assertEqual(len(manifest['cases']), 12)
        self.assertEqual(len(manifest['repeated_cases']), 3)
        self.assertEqual(len({c['case']['id'] for c in manifest['cases']}), 12)

    def test_judgments_require_exact_claims_source_evidence_and_all_measurements(self):
        packet, judgment = self.fixture()
        self.assertTrue(focused.validate_judgment(judgment, packet))
        for mutate in [
            lambda j: j['claims'][0].update(text='different'),
            lambda j: j['claims'][0].update(index=1),
            lambda j: j['claims'][0].update(evidence=[{'start': 0, 'end': 6, 'quote': '한글'}]),
            lambda j: j['claims'][0]['checks'].pop('atomicity'),
            lambda j: j['claims'][0]['checks']['atomicity'].update(status='unknown'),
            lambda j: j['claims'][0]['checks']['atomicity'].update(status='fail'),
            lambda j: j['claims'][0]['checks']['atomicity'].update(status='unresolved'),
            lambda j: j.update(inventory=[]),
            lambda j: j['inventory'][0].update(claim_indices=[]),
            lambda j: j['claims'][0].update(uncertainty_flags=['contested']),
        ]:
            changed = copy.deepcopy(judgment)
            mutate(changed)
            with self.assertRaises(AssertionError):
                focused.validate_judgment(changed, packet)
        judgment['claims'][0]['checks']['atomicity']['status'] = 'unresolved'
        judgment['verdict'] = 'unresolved'
        self.assertTrue(focused.validate_judgment(judgment, packet))
