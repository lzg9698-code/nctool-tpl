"""Retired Rust NC sources remain recoverable by original bytes."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import tarfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
ARCHIVE = ROOT / 'docs/archive/legacy/rust-nc-workbench.tar.gz'
MANIFEST = ARCHIVE.with_name('rust-nc-workbench.manifest.json')


class RustNcArchiveTests(unittest.TestCase):
    def test_every_member_matches_the_original_source_manifest(self):
        body = json.loads(MANIFEST.read_text(encoding='utf-8'))
        self.assertEqual(hashlib.sha256(ARCHIVE.read_bytes()).hexdigest(), body['archive_sha256'])
        with tarfile.open(ARCHIVE, 'r:gz') as tar:
            members = tar.getmembers()
            self.assertEqual(len(members), len(body['files']))
            self.assertEqual({member.name for member in members}, set(body['files']))
            for member in members:
                with self.subTest(path=member.name):
                    self.assertTrue(member.isfile())
                    path = PurePosixPath(member.name)
                    self.assertFalse(path.is_absolute())
                    self.assertNotIn('..', path.parts)
                    raw = tar.extractfile(member).read()
                    self.assertEqual(len(raw), body['files'][member.name]['bytes'])
                    self.assertEqual(hashlib.sha256(raw).hexdigest(), body['files'][member.name]['sha256'])

    def test_latest_removal_archive_preserves_exact_working_tree_sources(self):
        archive = ROOT / 'docs/archive/legacy/rust-nc-removal-20261016.tar.gz'
        manifest = archive.with_name('rust-nc-removal-20261016.manifest.json')
        body = json.loads(manifest.read_text(encoding='utf-8'))
        self.assertEqual(hashlib.sha256(archive.read_bytes()).hexdigest(), body['archive_sha256'])
        with tarfile.open(archive, 'r:gz') as tar:
            members = tar.getmembers()
            self.assertEqual({member.name for member in members}, set(body['files']))
            self.assertEqual(len(members), len(body['files']))
            for member in members:
                with self.subTest(path=member.name):
                    self.assertTrue(member.isfile())
                    path = PurePosixPath(member.name)
                    self.assertFalse(path.is_absolute())
                    self.assertNotIn('..', path.parts)
                    raw = tar.extractfile(member).read()
                    expected = body['files'][member.name]
                    self.assertEqual(len(raw), expected['bytes'])
                    self.assertEqual(hashlib.sha256(raw).hexdigest(), expected['sha256'])
        for name in ['plugins/nc/src/lib.rs', 'plugins/process/src/lib.rs',
                     'ui/src/plugins/NcViews.tsx', 'cli/src/composition.rs',
                     'ui/src/styles.css', 'scripts/browser_v2.cjs']:
            self.assertIn(name, body['files'])
        self.assertFalse(any(name.startswith(('memory/', 'ui/dist/', 'cli/ui/', 'docs/archive/'))
                             for name in body['files']))

    def test_retired_modules_and_build_context_are_available(self):
        files = json.loads(MANIFEST.read_text(encoding='utf-8'))['files']
        for name in ['plugins/nc/src/lib.rs', 'plugins/process/src/lib.rs',
                     'plugins/sync-turning/src/lib.rs', 'plugins/undercut/src/lib.rs',
                     'ui/src/plugins/TurningWorkbench.tsx', 'cli/src/composition.rs',
                     'Cargo.toml', 'Cargo.lock', 'ui/package-lock.json',
                     'scripts/browser_v2.cjs', '.github/workflows/release.yml']:
            self.assertIn(name, files)
        self.assertFalse(any(name.startswith(('target/', 'shaft_programmer/', 'nc/')) for name in files))


if __name__ == '__main__':
    unittest.main()
