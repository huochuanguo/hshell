"""Keep scoped output profiling from counting an active caller's inclusive costs."""
from pathlib import Path
import tempfile
import unittest
from callgrind_summary import summarize

PROFILE = '''positions: line
events: Ir
summary: 49
ob=(1) client
fn=(1) dispatch
1 17
cob=(2) libvte
cfn=(2) parse
calls=0 1
1 999
ob=(2)
fn=(2)
1 33
totals: 50
'''


class ScopedProfileTests(unittest.TestCase):
    def test_zero_calls_inclusive_cost_is_excluded_and_trailing_total_used(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'profile'
            path.write_text(PROFILE)
            result = summarize(path)
        self.assertEqual(result['totalInstructions'], 50)
        self.assertEqual({row['object']: row['instructions'] for row in result['objects']},
                         {'client': 17, 'libvte': 33})

    def test_rejects_incomplete_self_costs(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'profile'
            path.write_text(PROFILE.replace('totals: 50', 'totals: 51'))
            with self.assertRaises(AssertionError):
                summarize(path)


if __name__ == '__main__':
    unittest.main()
