#!/usr/bin/env python3
"""Tests for the local development tooling in scripts/dev (no network, no deletion)."""
from __future__ import annotations

import json
from pathlib import Path, PurePosixPath
import subprocess
import shutil
import tempfile
import sys
import unittest
from unittest.mock import patch
import importlib.util
from types import SimpleNamespace

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
    def test_truncated_context_includes_suffix_within_limit(self) -> None:
        spec = importlib.util.spec_from_file_location("session_fixture", DEV / "session_context.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with patch.object(module, "status_summary", return_value=("HEAD", "x" * 7000)):
            self.assertLessEqual(len(module.build()), module.LIMIT)

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
    def setUp(self) -> None:
        self.fixture = tempfile.TemporaryDirectory(prefix="clean-target-tests-")
        self.addCleanup(self.fixture.cleanup)
        self.root = Path(self.fixture.name)
        script = self.root / "scripts" / "dev" / "clean_targets.py"
        script.parent.mkdir(parents=True)
        shutil.copy2(DEV / "clean_targets.py", script)
        (self.root / "target" / "debug").mkdir(parents=True)
        (self.root / "target" / "platform-default").mkdir()

    def invoke(self, *args: str):
        return run(sys.executable, str(self.root / "scripts/dev/clean_targets.py"), *args)

    def module(self):
        spec = importlib.util.spec_from_file_location("clean_target_fixture", self.root / "scripts/dev/clean_targets.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_missing_target_reports_without_crashing(self) -> None:
        shutil.rmtree(self.root / "target")
        result = self.invoke("--days", "0")
        self.assertEqual(result.returncode, 2)
        self.assertNotIn("Traceback", result.stderr)

    def test_no_legacy_debug_directory_is_required(self) -> None:
        (self.root / "target/debug").rmdir()
        result = self.invoke("--delete", "debug")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.root / "target/debug").exists())

    def test_running_and_unknown_processes_are_kept(self) -> None:
        module = self.module()
        candidate = self.root / "target" / "old-preview"
        candidate.mkdir()
        with patch.object(module, "referenced_names", return_value=(set(), set())):
            for processes in ([candidate / "nctool"], None):
                with patch.object(module, "running_binaries", return_value=processes):
                    candidates, kept = module.scan(0)
                    self.assertNotIn("old-preview", {e["name"] for e in candidates})
                    self.assertIn("old-preview", {e["name"] for e in kept})

    def test_linux_resolves_executable_instead_of_process_name(self) -> None:
        module = self.module()
        executable = str(self.root / "target/old-preview/nctool")
        with patch.object(module.sys, "platform", "linux"), patch.object(module.subprocess, "run", return_value=SimpleNamespace(returncode=0, stdout="123 nctool\n")), patch.object(module.os, "readlink", return_value=executable):
            self.assertEqual(module.running_binaries(), [Path(executable)])

    def test_windows_process_query_preserves_unknown_paths_and_errors(self) -> None:
        module = self.module()
        executable = str(self.root / "target/old-preview/nctool.exe")
        cases = [
            (0, "[]", []),
            (0, json.dumps([{"ProcessId": 123, "ExecutablePath": executable}]), [Path(executable)]),
            (0, '[{"ProcessId":123,"ExecutablePath":null}]', None),
            (0, "", None),
            (0, "invalid json", None),
            (1, "[]", None),
        ]
        for code, output, expected in cases:
            with self.subTest(code=code, output=output):
                with patch.object(module.sys, "platform", "win32"), patch.object(module.subprocess, "run", return_value=SimpleNamespace(returncode=code, stdout=output)) as query:
                    self.assertEqual(module.running_binaries(), expected)
                    self.assertIn("-ErrorAction Stop", query.call_args.args[0][-1])

    def test_macos_ignores_library_mappings_and_rejects_ambiguity(self) -> None:
        module = self.module()
        executable = "/tmp/target/old-preview/nctool"
        cases = [
            ("n/usr/lib/libSystem.B.dylib\nn" + executable + "\n", [PurePosixPath(executable)]),
            ("n/usr/lib/libSystem.B.dylib\n", None),
            ("n" + executable + "\nn/tmp/other/nctool\n", None),
        ]
        for output, expected in cases:
            with self.subTest(output=output):
                results = [SimpleNamespace(returncode=0, stdout="123 /tmp/nctool\n"), SimpleNamespace(returncode=0, stdout=output)]
                with patch.object(module.sys, "platform", "darwin"), patch.object(module, "Path", PurePosixPath), patch.object(module.subprocess, "run", side_effect=results):
                    self.assertEqual(module.running_binaries(), expected)

    def test_windows_removal_uses_python39_callback(self) -> None:
        module = self.module()
        with patch.object(module.sys, "platform", "win32"), patch.object(module.shutil, "rmtree") as remove:
            module.force_remove(self.root / "target/debug")
            self.assertIn("onerror", remove.call_args.kwargs)
            self.assertNotIn("onexc", remove.call_args.kwargs)

    def test_dry_run_deletes_nothing(self) -> None:
        before = sorted(path.name for path in (self.root / "target").iterdir() if path.is_dir())
        result = self.invoke("--days", "0")
        self.assertEqual(result.returncode, 0, result.stderr)
        after = sorted(path.name for path in (self.root / "target").iterdir() if path.is_dir())
        self.assertEqual(before, after)
        self.assertIn("干跑", result.stdout)

    def test_protected_directories_are_never_candidates(self) -> None:
        result = self.invoke("--days", "0", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        names = {entry["name"] for entry in report["candidates"]}
        for protected in ("debug", "dependency-audit", "agent-tools"):
            self.assertNotIn(protected, names)
        self.assertLessEqual(len(names), len(list((self.root / "target").glob("*"))))

    def test_explicit_delete_refuses_protected_name(self) -> None:
        result = self.invoke(
            "--days",
            "0",
            "--delete",
            "debug",
        )
        self.assertNotEqual(result.returncode, 2)
        self.assertIn("跳过 debug", result.stderr)
        self.assertTrue((self.root / "target" / "debug").is_dir())


class CheckEnvTests(unittest.TestCase):
    def test_json_output_is_utf8_with_ascii_process_defaults(self) -> None:
        import os
        with patch.dict(os.environ, {"PYTHONIOENCODING": "ascii"}):
            result = run(sys.executable, str(DEV / "check_env.py"), "--json")
        self.assertIn(result.returncode, (0, 1), result.stderr)
        report = json.loads(result.stdout)
        self.assertTrue(any(item["name"].startswith("CLAUDE.md") for item in report))
        self.assertNotIn("�", result.stdout)

    def test_json_report_shape(self) -> None:
        result = run(sys.executable, str(DEV / "check_env.py"), "--json")
        self.assertIn(result.returncode, (0, 1), result.stderr)
        report = json.loads(result.stdout)
        self.assertTrue(any(item["name"].startswith("CLAUDE.md") for item in report))
        self.assertTrue(all({"name", "status", "required"} <= set(item) for item in report))


@unittest.skipUnless(shutil.which("node"), "Node.js is required")
class PreviewLaunchTests(unittest.TestCase):
    def test_fresh_checkout_generates_launch_without_settings_template(self) -> None:
        with tempfile.TemporaryDirectory(prefix="setup-launch-") as directory:
            root = Path(directory)
            script = root / "scripts/dev/setup_agent_env.mjs"
            script.parent.mkdir(parents=True)
            shutil.copy2(DEV / "setup_agent_env.mjs", script)
            (root / ".agents/skills").mkdir(parents=True)
            result = run("node", str(script), "--apply", "--with-launch", "--json")
            self.assertEqual(result.returncode, 0, result.stderr)
            config = json.loads((root / ".claude/launch.json").read_text(encoding="utf-8"))
            self.assertEqual([c["name"] for c in config["configurations"]], ["dev-template", "dev-math"])
            self.assertFalse((root / ".claude/settings.json").exists())

    def test_launch_check_exit_codes_match_missing_stale_and_ready(self) -> None:
        with tempfile.TemporaryDirectory(prefix="setup-exits-") as directory:
            root = Path(directory)
            script = root / "scripts/dev/setup_agent_env.mjs"
            script.parent.mkdir(parents=True)
            shutil.copy2(DEV / "setup_agent_env.mjs", script)
            args = ("node", str(script), "--with-launch", "--json")
            self.assertEqual(run(*args).returncode, 1)
            self.assertFalse((root / ".claude").exists())
            self.assertEqual(run(*args, "--apply").returncode, 0)
            self.assertEqual(run(*args).returncode, 0)
            launch = root / ".claude/launch.json"
            launch.write_text('{}', encoding="utf-8")
            self.assertEqual(run(*args).returncode, 1)
            self.assertEqual(launch.read_text(encoding="utf-8"), '{}')

    def test_plain_apply_creates_settings_without_template(self) -> None:
        with tempfile.TemporaryDirectory(prefix="setup-default-") as directory:
            root = Path(directory)
            script = root / "scripts/dev/setup_agent_env.mjs"
            script.parent.mkdir(parents=True)
            shutil.copy2(DEV / "setup_agent_env.mjs", script)
            (root / ".agents/skills").mkdir(parents=True)
            (root / "CLAUDE.md").write_text('@AGENTS.md\n', encoding="utf-8")
            (root / ".mcp.json").write_text('{"mcpServers":{}}', encoding="utf-8")
            result = run("node", str(script), "--apply", "--json")
            self.assertEqual(result.returncode, 0, result.stderr)
            settings = root / ".claude/settings.json"
            self.assertEqual(json.loads(settings.read_text(encoding="utf-8"))["permissions"], {"allow": [], "ask": []})
            original = settings.read_bytes()
            self.assertEqual(run("node", str(script), "--apply", "--json").returncode, 0)
            self.assertEqual(settings.read_bytes(), original)

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
