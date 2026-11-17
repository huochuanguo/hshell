#!/usr/bin/env python3
"""Check that invalid tags or missing notes cannot reach release publication."""
from pathlib import Path
import tempfile
import unittest
from release_metadata import metadata


class ReleaseMetadataTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / 'crates/native-linux').mkdir(parents=True)
        (self.root / 'crates/native-linux/Cargo.toml').write_text('[package]\nversion = "0.3.1"\n')
        (self.root / 'docs/releases').mkdir(parents=True)
        self.notes = self.root / 'docs/releases/0.3.1.md'
        self.notes.write_text('Release notes\n')

    def test_valid_tag_uses_native_version_and_notes(self):
        self.assertEqual(metadata('v0.3.1', self.root), {
            'version': '0.3.1', 'package': 'hshell_0.3.1_amd64.deb',
            'notes': 'docs/releases/0.3.1.md',
        })

    def test_mismatched_tag_is_rejected(self):
        with self.assertRaises(ValueError):
            metadata('v0.3.2', self.root)

    def test_unsafe_or_unreleased_tag_is_rejected(self):
        for tag in ('main', '0.3.1', 'v00.3.1', 'v0.3.1-rc.1', 'v0.3.1\nx=y', 'v0.3.1;false'):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                metadata(tag, self.root)

    def test_missing_or_empty_notes_are_rejected(self):
        self.notes.unlink()
        with self.assertRaises(ValueError):
            metadata('v0.3.1', self.root)
        self.notes.write_text(' \n')
        with self.assertRaises(ValueError):
            metadata('v0.3.1', self.root)


if __name__ == '__main__':
    unittest.main()
