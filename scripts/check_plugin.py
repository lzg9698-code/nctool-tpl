#!/usr/bin/env python3
"""Plugin-author acceptance against the real host, using an explicit fixture plan.

Runs a COPY of the package in disposable home/workspace directories. A plugin is
ordinary same-user code, not an OS sandbox. Use test-only inputs and dependencies.
Reports contain check names/error codes, never request/result/config bodies.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request


class CheckFailure(Exception):
    pass


def validate_plan(plan):
    if not isinstance(plan, dict) or type(plan.get("timeout_ms")) is not int or not 500 <= plan["timeout_ms"] <= 10000:
        raise CheckFailure("invalid_plan_timeout")
    if type(plan.get("late_wait_ms")) is not int or not plan["timeout_ms"] <= plan["late_wait_ms"] <= 30000:
        raise CheckFailure("invalid_plan_late_wait")
    if not isinstance(plan.get("identity_field"), str) or not plan["identity_field"]:
        raise CheckFailure("process_identity_field_required")
    for name in ("success", "invalid", "cancel", "timeout", "unresponsive"):
        case = plan.get(name)
        if not isinstance(case, dict) or not isinstance(case.get("action"), str) or "input" not in case:
            raise CheckFailure("invalid_plan_case_" + name)
    if not isinstance(plan["success"].get("expected"), dict):
        raise CheckFailure("success_expectation_required")
    callbacks = plan["success"].get("callback_actions")
    if not isinstance(callbacks, list) or not all(isinstance(a, str) for a in callbacks):
        raise CheckFailure("callback_expectation_required")
    return plan


def subset(actual, expected):
    if isinstance(expected, dict):
        return isinstance(actual, dict) and all(k in actual and subset(actual[k], v) for k, v in expected.items())
    return actual == expected


class Host:
    def __init__(self, binary, home, workspace):
        self.process = subprocess.Popen([str(binary), "--home", str(home), "--workspace", str(workspace), "ui", "--port", "0"],
                                        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, encoding="utf-8",
                                        start_new_session=os.name != "nt",
                                        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        urls = queue.Queue()
        self.startup_codes = []
        def drain():
            for line in self.process.stderr:
                code = re.search(r'"code"\s*:\s*"([A-Za-z0-9_.-]{1,128})"', line)
                if code:
                    self.startup_codes.append(code[1])
                match = re.search(r"http://127\.0\.0\.1:\d+", line)
                if match:
                    urls.put(match[0])
        self.reader = threading.Thread(target=drain, daemon=True)
        self.reader.start()
        try:
            self.url = urls.get(timeout=30)
        except queue.Empty:
            self.close()
            raise CheckFailure("host_startup_" + (self.startup_codes[-1] if self.startup_codes else "failed")) from None

    def request(self, route, body=None):
        data = None if body is None else json.dumps(body, allow_nan=False).encode()
        request = urllib.request.Request(self.url + "/api/v2/" + route, data=data,
                                         headers={"Origin":self.url,"Content-Type":"application/json"})
        try:
            response = urllib.request.urlopen(request, timeout=20)
        except urllib.error.HTTPError as error:
            response = error
        envelope = json.loads(response.read())
        if envelope.get("ok") is not True:
            raise CheckFailure(envelope.get("error", {}).get("code", "invalid_envelope"))
        return envelope["data"]

    def start(self, case):
        return self.request("runs", {"action":case["action"], "input":case["input"], "title":"Plugin acceptance fixture"})["id"]

    def finished(self, run_id):
        deadline = time.monotonic() + 40
        while time.monotonic() < deadline:
            run = self.request("runs/" + run_id)
            if run["status"] not in ("queued", "running"):
                return run
            time.sleep(0.025)
        raise CheckFailure("test_run_deadline")

    def close(self):
        if self.process.poll() is None:
            # Kill only our host/process tree, including its disposable plugin workers.
            if os.name == "nt":
                subprocess.run(["taskkill", "/PID", str(self.process.pid), "/T", "/F"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
            else:
                os.killpg(self.process.pid, signal.SIGTERM)
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
            if os.name != "nt":
                try:
                    os.killpg(self.process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        self.reader.join(timeout=1)
        self.process.stderr.close()


def verify(binary, source, plan, python=None, dependencies=()):
    validate_plan(plan)
    checks = []
    with tempfile.TemporaryDirectory(prefix="nctool-plugin-check-") as temporary:
        root = Path(temporary)
        package, home, workspace = root / "package", root / "home", root / "workspace"
        # Reject links before copytree: fixture plans must not pull in outside data.
        if source.is_symlink() or any(p.is_symlink() for p in source.rglob("*")):
            raise CheckFailure("plugin_symlink")
        files = list(source.rglob("*"))
        if len(files) > 4096 or sum(p.stat().st_size for p in files if p.is_file()) > 64 * 1024 * 1024:
            raise CheckFailure("package_limit")
        shutil.copytree(source, package, ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
        manifest_path = package / "plugin.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if manifest["descriptor"].get("requires") and not plan["success"]["callback_actions"]:
            raise CheckFailure("callback_expectation_required")
        manifest.setdefault("limits", {}).update(timeout_ms=plan["timeout_ms"], max_concurrency=2)
        if python:
            manifest["command"][0] = str(python)
        manifest_path.write_text(json.dumps(manifest, ensure_ascii=False), encoding="utf-8")
        home.mkdir()
        workspace.mkdir()
        commands=[]
        for dependency in dependencies:
            dependency_id=json.loads((dependency/"plugin.json").read_text(encoding="utf-8"))["descriptor"]["id"]
            commands.extend((["plugins","install",str(dependency)],["plugins","enable",dependency_id]))
        commands.extend((["plugins","install",str(package)],["plugins","enable",manifest["descriptor"]["id"]]))
        for args in commands:
            result = subprocess.run([str(binary),"--home",str(home),"--workspace",str(workspace),*args], capture_output=True,
                                    creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0, timeout=30)
            if result.returncode:
                raise CheckFailure("package_install_or_enable_failed")
        if "config" in plan:
            config_path=home/"config.json"
            config=json.loads(config_path.read_text(encoding="utf-8"))
            config.setdefault("plugins",{})[manifest["descriptor"]["id"]]=plan["config"]
            config_path.write_text(json.dumps(config,ensure_ascii=False,allow_nan=False),encoding="utf-8")
        host = Host(binary, home, workspace)
        try:
            checks.append("manifest_initialize_describe_and_dependencies")
            def success():
                run_id = host.start(plan["success"])
                run = host.finished(run_id)
                if run["status"] != "succeeded" or not subset(run["result"]["data"], plan["success"]["expected"]):
                    raise CheckFailure("success_result_mismatch")
                calls = host.request("diagnostics")["calls"]
                called = {c["action"] for c in calls if c["trace_id"] == run_id and c["depth"] > 0}
                if not set(plan["success"]["callback_actions"]).issubset(called):
                    raise CheckFailure("host_callback_not_observed")
                identity = run["result"]["data"].get(plan["identity_field"])
                if identity is None:
                    raise CheckFailure("process_identity_missing")
                return identity
            initial_identity = success()
            checks.append("success_output_schema_and_reverse_host_callback" if plan["success"]["callback_actions"] else "success_output_schema_callback_not_applicable")
            try:
                invalid = host.finished(host.start(plan["invalid"]))
                if invalid["status"] != "failed" or invalid["error"]["code"] != "invalid_input":
                    raise CheckFailure("invalid_input_not_rejected")
            except CheckFailure as error:
                if str(error) != "invalid_input":
                    raise
            checks.append("invalid_input_schema")
            cancelled = host.start(plan["cancel"])
            time.sleep(0.1)
            host.request("runs/" + cancelled + "/cancel", {})
            if host.finished(cancelled)["status"] != "cancelled":
                raise CheckFailure("cancellation_failed")
            time.sleep(0.2)
            checks.append("request_cancellation")
            timed = host.finished(host.start(plan["timeout"]))
            if timed["status"] != "failed" or timed["error"]["code"] != "plugin_timeout":
                raise CheckFailure("timeout_not_enforced")
            time.sleep(plan["late_wait_ms"] / 1000)
            if success() != initial_identity:
                raise CheckFailure("late_response_restarted_live_transport")
            checks.append("timeout_late_response_and_same_process_reuse")
            abandoned = [host.start(plan["unresponsive"]) for _ in range(2)]
            time.sleep(0.1)
            for run_id in abandoned:
                host.request("runs/" + run_id + "/cancel", {})
            for run_id in abandoned:
                if host.finished(run_id)["status"] != "cancelled":
                    raise CheckFailure("saturation_cancellation_failed")
            # Explicit, independent developer recovery probes, never automatic retries
            # of an unacknowledged business request.
            for _ in range(10):
                time.sleep(0.2)
                try:
                    recovered = success()
                    break
                except CheckFailure as error:
                    if str(error) != "success_result_mismatch":
                        raise
            else:
                raise CheckFailure("saturation_recovery_failed")
            if recovered == initial_identity:
                raise CheckFailure("process_restart_not_observed")
            checks.append("all_abandoned_slots_process_restart_and_recovery")
            return {"status":"passed","checks":checks,"protocol_version":manifest["descriptor"]["protocol_version"],
                    "binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),"scope":"disposable_test_package"}
        except CheckFailure as error:
            error.completed_checks = list(checks)
            raise
        finally:
            host.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--python", type=Path, help="override Python fixture command in the copied package")
    parser.add_argument("--dependency", type=Path, action="append", default=[], help="local test dependency package, installed only into the disposable home")
    args = parser.parse_args()
    try:
        report = verify(args.binary.resolve(), args.source.resolve(), json.loads(args.plan.read_text(encoding="utf-8")), args.python,[p.resolve() for p in args.dependency])
    except Exception as error:
        # Transport/system errors may contain a URL or user data; expose only a class/code.
        report = {"status":"failed","error_code":str(error) if isinstance(error,CheckFailure) else type(error).__name__,
                  "checks":getattr(error,"completed_checks",[])}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
