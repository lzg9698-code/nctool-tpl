"""Retired workbench must remain recoverable without active duplicate entry points."""
import hashlib
import json
from pathlib import Path
import tarfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
ARCHIVE = ROOT / 'docs/archive/legacy/workbench-fixed-scheme.tar.gz'
MANIFEST = ARCHIVE.with_name('workbench-fixed-scheme.manifest.json')


class WorkbenchArchiveTest(unittest.TestCase):
    def test_archive_matches_manifest_and_every_member(self):
        body = json.loads(MANIFEST.read_text(encoding='utf-8'))
        self.assertEqual(hashlib.sha256(ARCHIVE.read_bytes()).hexdigest(), body['archive_sha256'])
        with tarfile.open(ARCHIVE, 'r:gz') as tar:
            members = tar.getmembers()
            self.assertEqual(len(members), len(body['files']))
            self.assertEqual({member.name for member in members}, set(body['files']))
            for member in members:
                with self.subTest(path=member.name):
                    self.assertTrue(member.isfile())
                    self.assertFalse(Path(member.name).is_absolute())
                    self.assertNotIn('..', Path(member.name).parts)
                    raw = tar.extractfile(member).read()
                    self.assertEqual(len(raw), body['files'][member.name]['size'])
                    self.assertEqual(hashlib.sha256(raw).hexdigest(), body['files'][member.name]['sha256'])

    def test_old_entry_points_are_retired(self):
        body = json.loads(MANIFEST.read_text(encoding='utf-8'))
        for name in body['retired_paths']:
            with self.subTest(path=name):
                self.assertFalse((ROOT / name).exists())

    def test_recovery_context_and_prebuilt_page_are_preserved(self):
        body = json.loads(MANIFEST.read_text(encoding='utf-8'))
        files = body['files']
        for name in ['workbench/domain.py', 'workbench/post.py', 'workbench/server.py',
                     'workbench/README.md', 'workbench/static/workbench.html',
                     'workbench/static/source-manifest.json', 'ui/package-lock.json',
                     'ui/src/workbench/main.tsx', 'scripts/build_workbench.mjs']:
            self.assertIn(name, files)
        for name in body['context_paths']:
            self.assertTrue((ROOT / name).is_file())
        self.assertTrue(any(name.startswith('workbench/static/assets/') for name in files))
        self.assertFalse(any(name.startswith('target/') or '__pycache__' in name for name in files))
        with tarfile.open(ARCHIVE, 'r:gz') as tar:
            sources = json.loads(tar.extractfile('workbench/static/source-manifest.json').read())
            for name, expected in sources['files'].items():
                with self.subTest(source=name):
                    self.assertEqual(hashlib.sha256(tar.extractfile(name).read()).hexdigest(), expected)


if __name__ == '__main__':
    unittest.main()
