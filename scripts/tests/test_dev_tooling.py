#!/usr/bin/env python3
"""Tests for the local development tooling in scripts/dev (no network, no deletion)."""
from __future__ import annotations

import json
from pathlib import Path
import subprocess
import shutil
import tempfile
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
DEV = ROOT / "scripts" / "dev"


def run(*command: str, stdin: str | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=ROOT,
        input=stdin,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=300,
    )


class SessionContextTests(unittest.TestCase):
    def test_emits_session_start_payload(self) -> None:
        result = run(sys.executable, str(DEV / "session_context.py"))
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        output = payload["hookSpecificOutput"]
        self.assertEqual(output["hookEventName"], "SessionStart")
        context = output["additionalContext"]
        self.assertIn("NCtool 会话基线", context)
        self.assertLessEqual(len(context), 6000)

    def test_context_never_claims_a_missing_brief(self) -> None:
        result = run(sys.executable, str(DEV / "session_context.py"))
        context = json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("项目简报", context)


class CleanTargetTests(unittest.TestCase):
    def test_dry_run_deletes_nothing(self) -> None:
        before = sorted(path.name for path in (ROOT / "target").iterdir() if path.is_dir())
        result = run(sys.executable, str(DEV / "clean_targets.py"), "--days", "0")
        self.assertEqual(result.returncode, 0, result.stderr)
        after = sorted(path.name for path in (ROOT / "target").iterdir() if path.is_dir())
        self.assertEqual(before, after)
        self.assertIn("干跑", result.stdout)

    def test_protected_directories_are_never_candidates(self) -> None:
        result = run(sys.executable, str(DEV / "clean_targets.py"), "--days", "0", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        names = {entry["name"] for entry in report["candidates"]}
        for protected in ("debug", "dependency-audit", "agent-tools"):
            self.assertNotIn(protected, names)
        self.assertLessEqual(len(names), len(list((ROOT / "target").glob("*"))))

    def test_explicit_delete_refuses_protected_name(self) -> None:
        result = run(
            sys.executable,
            str(DEV / "clean_targets.py"),
            "--days",
            "0",
            "--delete",
            "debug",
        )
        self.assertNotEqual(result.returncode, 2)
        self.assertIn("跳过 debug", result.stderr)
        self.assertTrue((ROOT / "target" / "debug").is_dir())


class CheckEnvTests(unittest.TestCase):
    def test_json_report_shape(self) -> None:
        result = run(sys.executable, str(DEV / "check_env.py"), "--json")
        self.assertIn(result.returncode, (0, 1), result.stderr)
        report = json.loads(result.stdout)
        self.assertTrue(any(item["name"].startswith("CLAUDE.md") for item in report))
        self.assertTrue(all({"name", "status", "required"} <= set(item) for item in report))


@unittest.skipUnless(shutil.which("node"), "Node.js is required")
class PreviewLaunchTests(unittest.TestCase):
    def test_missing_binary_check_is_read_only_and_variants_are_isolated(self) -> None:
        with tempfile.TemporaryDirectory(prefix="preview-launch-") as directory:
            root = Path(directory)
            scripts = root / "scripts" / "dev"
            scripts.mkdir(parents=True)
            shutil.copy2(DEV / "preview_ui.mjs", scripts / "preview_ui.mjs")
            reports = []
            for variant in ("default", "math"):
                result = run("node", str(scripts / "preview_ui.mjs"), variant, "--check")
                self.assertEqual(result.returncode, 0, result.stderr)
                report = json.loads(result.stdout)
                self.assertFalse(report["exists"])
                self.assertIn("platform-" + variant, report["binary"])
                self.assertIn("--locked", report["build"])
                self.assertIn("--home", report["launch"])
                self.assertIn("--workspace", report["launch"])
                reports.append(report)
            self.assertNotIn("--features", reports[0]["build"])
            self.assertEqual(reports[1]["build"][-2:], ["--features", "math"])
            self.assertNotEqual(reports[0]["binary"], reports[1]["binary"])
            self.assertFalse((root / "target").exists())

    def test_invalid_variant_fails_before_creating_files(self) -> None:
        result = run("node", str(DEV / "preview_ui.mjs"), "nc", "--check")
        self.assertEqual(result.returncode, 2)
        self.assertIn("Usage:", result.stderr)


if __name__ == "__main__":
    unittest.main()
