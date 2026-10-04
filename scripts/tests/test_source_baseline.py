import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "source_baseline.py"
SPEC = importlib.util.spec_from_file_location("source_baseline", SCRIPT)
baseline = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(baseline)


class SourceBaselineTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "repo"
        self.root.mkdir()
        self.destination = Path(self.temp.name) / "snapshot"
        self.git("init", "-q")
        self.git("config", "user.email", "baseline@example.invalid")
        self.git("config", "user.name", "Baseline test")
        (self.root / "tracked.txt").write_text("original\n")
        (self.root / "deleted.txt").write_text("delete me\n")
        (self.root / ".gitignore").write_text("ignored/\n")
        self.git("add", ".")
        self.git("commit", "-qm", "Initial")

    def git(self, *args):
        return baseline.git(self.root, *args)

    def test_capture_preserves_staged_unstaged_untracked_and_deletions(self):
        tracked = self.root / "tracked.txt"
        tracked.write_text("staged\n")
        self.git("add", "tracked.txt")
        tracked.write_text("unstaged\n")
        (self.root / "deleted.txt").unlink()
        (self.root / "new file.txt").write_text("new\n")
        (self.root / "ignored").mkdir()
        (self.root / "ignored/private.txt").write_text("excluded\n")
        before = self.git("status", "--porcelain=v1")
        manifest = baseline.capture(self.root, self.destination)
        self.assertEqual(before, self.git("status", "--porcelain=v1"))
        self.assertEqual({entry["path"] for entry in manifest["files"]},
                         {".gitignore", "tracked.txt", "new file.txt"})
        self.assertIn(b"staged", (self.destination / "staged.patch").read_bytes())
        self.assertIn(b"unstaged", (self.destination / "unstaged.patch").read_bytes())
        self.assertIn(b"deleted.txt", (self.destination / "status.txt").read_bytes())
        self.assertEqual(manifest, baseline.verify(self.destination))

    def test_refuses_to_overwrite_existing_snapshot(self):
        baseline.capture(self.root, self.destination)
        with self.assertRaises(FileExistsError):
            baseline.capture(self.root, self.destination)

    def test_concurrent_edit_does_not_produce_verified_manifest(self):
        original = baseline.git_evidence
        calls = 0

        def edited_evidence(root):
            nonlocal calls
            calls += 1
            if calls == 2:
                (root / "tracked.txt").write_text("concurrent edit\n")
            return original(root)

        with patch.object(baseline, "git_evidence", edited_evidence):
            with self.assertRaisesRegex(ValueError, "state changed"):
                baseline.capture(self.root, self.destination)
        self.assertFalse((self.destination / "manifest.json").exists())

    def test_corrupt_archive_is_rejected(self):
        baseline.capture(self.root, self.destination)
        with (self.destination / "sources.tar.gz").open("ab") as stream:
            stream.write(b"corruption")
        with self.assertRaisesRegex(ValueError, "archive checksum"):
            baseline.verify(self.destination)

    def test_corrupt_evidence_is_rejected(self):
        baseline.capture(self.root, self.destination)
        (self.destination / "staged.patch").write_text("changed")
        with self.assertRaisesRegex(ValueError, "Evidence checksum"):
            baseline.verify(self.destination)

    def test_missing_source_manifest_entry_is_rejected(self):
        baseline.capture(self.root, self.destination)
        path = self.destination / "manifest.json"
        manifest = json.loads(path.read_text())
        manifest["files"].append({"path": "missing.txt", "bytes": 0, "sha256": ""})
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, "incomplete"):
            baseline.verify(self.destination)


if __name__ == "__main__":
    unittest.main()
