import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("audit_dependencies", Path(__file__).resolve().parents[1] / "audit_dependencies.py")
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)


class DependencyAuditTests(unittest.TestCase):
    def test_main_audits_only_current_lockfiles_and_preserves_failure_gate(self):
        cargo_report = json.dumps({"vulnerabilities": {"found": False}})
        npm_report = json.dumps({"auditReportVersion": 2, "metadata": {
            "vulnerabilities": {"high": 0, "critical": 0}}})
        for npm_code, npm_text, expected in ((0, npm_report, 0), (2, "network error", 2)):
            with self.subTest(npm_code=npm_code), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / "ui").mkdir()
                (root / "Cargo.lock").write_text("cargo lock", encoding="utf-8")
                (root / "ui/package-lock.json").write_text("npm lock", encoding="utf-8")
                output = root / "reports"
                reports = [subprocess.CompletedProcess([], 0, cargo_report, ""),
                           subprocess.CompletedProcess([], npm_code, npm_text, "npm stderr")]
                with patch.object(audit, "ROOT", root), \
                     patch.object(audit.subprocess, "run", side_effect=reports) as run:
                    self.assertEqual(audit.main(["--output", str(output)]), expected)
                commands = [call.args[0] for call in run.call_args_list]
                self.assertEqual(len(commands), 2)
                self.assertIn(str(root / "Cargo.lock"), commands[0])
                self.assertIn(str(root / "ui"), commands[1])
                summary = json.loads((output / "summary.json").read_text(encoding="utf-8"))
                self.assertEqual([item["name"] for item in summary["audits"]], ["cargo", "ui"])
                self.assertEqual(set(summary["lockfile_sha256"]),
                                 {"Cargo.lock", str(Path("ui/package-lock.json"))})
                self.assertTrue(summary["lockfiles_unchanged"])
                self.assertEqual((output / "ui.json").read_text(encoding="utf-8"), npm_text)
                self.assertEqual((output / "ui.stderr.txt").read_text(encoding="utf-8"), "npm stderr")

    def test_main_rejects_lockfile_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "ui").mkdir()
            (root / "Cargo.lock").write_text("before", encoding="utf-8")
            (root / "ui/package-lock.json").write_text("npm lock", encoding="utf-8")
            output = root / "reports"

            def changed_lockfile(name, *args):
                (root / "Cargo.lock").write_text("after", encoding="utf-8")
                return {"name": name, "status": "ok"}

            with patch.object(audit, "ROOT", root), \
                 patch.object(audit, "run_audit", side_effect=changed_lockfile):
                self.assertEqual(audit.main(["--output", str(output)]), 2)
            summary = json.loads((output / "summary.json").read_text(encoding="utf-8"))
            self.assertFalse(summary["lockfiles_unchanged"])

    def test_findings_are_not_success_even_with_zero_exit(self):
        for code in (0, 1):
            self.assertEqual(audit.classify("cargo", code, json.dumps({"vulnerabilities": {"found": True}})), "findings")
            for severity in ("high", "critical"):
                counts = {"high": 0, "critical": 0, severity: 1}
                report = {"auditReportVersion": 2, "metadata": {"vulnerabilities": counts}}
                self.assertEqual(audit.classify("npm", code, json.dumps(report)), "findings")

    def test_errors_and_incomplete_reports_never_pass(self):
        for kind in ("cargo", "npm"):
            for text in ("", "proxy error", "[]", "{}", '{"error":{"code":"E401"}}'):
                for code in (0, 1, 2):
                    self.assertEqual(audit.classify(kind, code, text), "error")

    def test_npm_lower_severity_remains_visible_but_does_not_gate(self):
        report = {"auditReportVersion": 2, "metadata": {"vulnerabilities": {"high": 0, "critical": 0, "moderate": 3}}}
        self.assertEqual(audit.classify("npm", 0, json.dumps(report)), "ok")
        self.assertEqual(audit.classify("npm", 1, json.dumps(report)), "error")

    def test_malformed_nested_metadata_is_a_tool_error(self):
        for metadata in (None, [], "unexpected", {"vulnerabilities": None},
                         {"vulnerabilities": []}, {"vulnerabilities": {"high": True, "critical": 0}}):
            self.assertEqual(audit.classify("npm", 0, json.dumps({"auditReportVersion": 2, "metadata": metadata})), "error")

    def test_report_and_stderr_preserved_for_findings(self):
        text = json.dumps({"vulnerabilities": {"found": True}})
        completed = subprocess.CompletedProcess(["cargo"], 1, text, "warning details")
        with tempfile.TemporaryDirectory() as directory, patch.object(audit.subprocess, "run", return_value=completed):
            output = Path(directory)
            result = audit.run_audit("cargo", "cargo", ["cargo", "audit"], output)
            self.assertEqual(result["status"], "findings")
            self.assertEqual((output / "cargo.json").read_text(encoding="utf-8"), text)
            self.assertEqual((output / "cargo.stderr.txt").read_text(encoding="utf-8"), "warning details")

    def test_timeout_preserves_partial_stdout_and_stderr_without_passing(self):
        error = subprocess.TimeoutExpired(
            ["cargo", "audit"], 300, output=b'{"partial":true}', stderr=b"fetch started"
        )
        with tempfile.TemporaryDirectory() as directory, patch.object(audit.subprocess, "run", side_effect=error):
            output = Path(directory)
            result = audit.run_audit("cargo", "cargo", ["cargo", "audit"], output)
            self.assertEqual(result["status"], "error")
            self.assertIsNone(result["returncode"])
            self.assertEqual((output / "cargo.json").read_text(encoding="utf-8"), '{"partial":true}')
            stderr = (output / "cargo.stderr.txt").read_text(encoding="utf-8")
            self.assertIn("fetch started", stderr)
            self.assertIn("timed out", stderr)

    def test_missing_tool_and_timeout_are_recorded(self):
        for error in (FileNotFoundError("missing cargo"), subprocess.TimeoutExpired(["cargo"], 300)):
            with tempfile.TemporaryDirectory() as directory, patch.object(audit.subprocess, "run", side_effect=error):
                output = Path(directory)
                result = audit.run_audit("cargo", "cargo", ["cargo", "audit"], output)
                self.assertEqual(result["status"], "error")
                self.assertIsNone(result["returncode"])
                self.assertTrue((output / "cargo.stderr.txt").read_text(encoding="utf-8"))
