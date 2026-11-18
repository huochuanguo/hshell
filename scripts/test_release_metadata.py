#!/usr/bin/env python3
"""Check that invalid tags or missing notes cannot reach release publication."""
import json
from pathlib import Path
import tempfile
import unittest
from release_metadata import metadata


class ReleaseMetadataTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        for folder in ('crates/native-linux', 'src-tauri'):
            (self.root / folder).mkdir(parents=True)
            (self.root / folder / 'Cargo.toml').write_text('[package]\nversion = "0.3.1"\n')
        for path in ('src-tauri/tauri.conf.json', 'package.json'):
            (self.root / path).write_text(json.dumps({'version': '0.3.1'}))
        (self.root / 'package-lock.json').write_text(json.dumps({
            'version': '0.3.1', 'packages': {'': {'version': '0.3.1'}},
        }))
        (self.root / 'docs/releases').mkdir(parents=True)
        self.notes = self.root / 'docs/releases/0.3.1.md'
        self.notes.write_text('Release notes\n')

    def test_valid_tag_uses_shared_version_and_notes(self):
        self.assertEqual(metadata('v0.3.1', self.root), {
            'version': '0.3.1', 'package': 'hshell_0.3.1_amd64.deb',
            'windows_package': 'HShell_0.3.1_x64-setup.exe',
            'notes': 'docs/releases/0.3.1.md',
        })

    def test_mismatched_tag_is_rejected(self):
        with self.assertRaises(ValueError):
            metadata('v0.3.2', self.root)

    def test_mismatched_windows_version_is_rejected(self):
        for name in ('src-tauri/Cargo.toml', 'src-tauri/tauri.conf.json', 'package.json', 'package-lock.json'):
            path = self.root / name
            original = path.read_text()
            with self.subTest(path=name):
                path.write_text(original.replace('0.3.1', '0.3.2'))
                with self.assertRaisesRegex(ValueError, name):
                    metadata('v0.3.1', self.root)
                path.write_text(original)

    def test_mismatched_lock_root_package_version_is_rejected(self):
        (self.root / 'package-lock.json').write_text(json.dumps({
            'version': '0.3.1', 'packages': {'': {'version': '0.3.2'}},
        }))
        with self.assertRaisesRegex(ValueError, 'root package'):
            metadata('v0.3.1', self.root)

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
