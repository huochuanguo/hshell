#!/usr/bin/env python3
"""Regression checks for soak evidence acceptance and false-pass prevention."""
import copy
import unittest

from validate_native_soak import check_lifecycle, check_resources


class SoakValidationTests(unittest.TestCase):
    def setUp(self):
        self.samples = [dict(seconds=seconds, pssMiB=110.7, rssMiB=208.9,
                             cpuPercentOneCore=33.5, fds=68, threads=15)
                        for seconds in range(0, 7201, 30)]
        self.manifest = dict(binarySha256='same-build')
        self.lifecycle = dict(
            binarySha256='same-build', diagnosticOnly=False, sshRemaining=0,
            fixture=dict(connections=76),
            result=dict(phase='complete', seconds=30, tabs=12, cycles=64,
                        sessions=0, hiddenOutputComplete=True, reclaimCalls=3,
                        partialLoad=True, inputP95=40, partialInputP95=20,
                        loopTickP95=28))

    def test_two_hour_resources_and_separate_lifecycle(self):
        summary = check_resources(self.samples, 7200, 7200, 8)
        self.assertEqual(summary['resourceGrowth']['pssMiB']['delta'], 0)
        check_lifecycle(self.lifecycle, self.manifest, 20)
        with self.assertRaises(AssertionError):
            check_lifecycle(self.lifecycle, self.manifest, 7200)

    def test_short_observation_is_rejected(self):
        with self.assertRaises(AssertionError):
            check_resources(self.samples[:-1], 7170, 7200, 8)

    def test_sample_gaps_and_counter_resets_are_rejected(self):
        with self.assertRaises(AssertionError):
            check_resources(self.samples[:50] + self.samples[55:], 7200, 7200, 8)
        self.samples[100]['cpuPercentOneCore'] = -100
        with self.assertRaises(AssertionError):
            check_resources(self.samples, 7200, 7200, 8)

    def test_memory_and_handle_growth_are_rejected(self):
        for key, value in [('pssMiB', 120), ('rssMiB', 220), ('fds', 69), ('threads', 16)]:
            with self.subTest(key=key):
                samples = copy.deepcopy(self.samples)
                for row in samples[-60:]:
                    row[key] = value
                with self.assertRaises(AssertionError):
                    check_resources(samples, 7200, 7200, 8)

    def test_wrong_binary_and_residual_connections_are_rejected(self):
        for key, value in [('binarySha256', 'other-build'), ('sshRemaining', 1),
                           ('diagnosticOnly', True)]:
            with self.subTest(key=key):
                report = copy.deepcopy(self.lifecycle)
                report[key] = value
                with self.assertRaises(AssertionError):
                    check_lifecycle(report, self.manifest, 20)

    def test_incomplete_lifecycle_is_rejected(self):
        for key, value in [('phase', 'stress-streaming'), ('cycles', 1),
                           ('partialLoad', False), ('hiddenOutputComplete', False),
                           ('sessions', 1), ('inputP95', 201)]:
            with self.subTest(key=key):
                report = copy.deepcopy(self.lifecycle)
                report['result'][key] = value
                with self.assertRaises(AssertionError):
                    check_lifecycle(report, self.manifest, 20)


if __name__ == '__main__':
    unittest.main()
