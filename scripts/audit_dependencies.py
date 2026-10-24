#!/usr/bin/env python3
"""Read-only Cargo/npm audit with preserved reports and fail-closed tool errors."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def classify(kind, returncode, stdout):
    """Return ok/findings/error; a missing or invalid report never passes."""
    try:
        report = json.loads(stdout)
    except (ValueError, TypeError):
        return "error"
    if not isinstance(report, dict) or "error" in report:
        return "error"
    if kind == "cargo":
        vulnerabilities = report.get("vulnerabilities")
        if not isinstance(vulnerabilities, dict) or not isinstance(vulnerabilities.get("found"), bool):
            return "error"
        if returncode not in (0, 1):
            return "error"
        if vulnerabilities["found"]:
            return "findings"
        return "ok" if returncode == 0 else "error"
    container = report.get("metadata")
    if not isinstance(container, dict):
        return "error"
    metadata = container.get("vulnerabilities")
    if report.get("auditReportVersion") != 2 or not isinstance(metadata, dict):
        return "error"
    high, critical = metadata.get("high"), metadata.get("critical")
    if type(high) is not int or type(critical) is not int or min(high, critical) < 0:
        return "error"
    if returncode not in (0, 1):
        return "error"
    if high or critical:
        return "findings"
    return "ok" if returncode == 0 else "error"


def run_audit(name, kind, command, output, root=ROOT):
    try:
        result = subprocess.run(command, cwd=root, capture_output=True, text=True,
                                encoding="utf-8", errors="replace", timeout=300)
        code, stdout, stderr = result.returncode, result.stdout, result.stderr
        status = classify(kind, code, stdout)
    except subprocess.TimeoutExpired as error:
        # Keep partial tool output as evidence, but a timeout never passes.
        def text(value):
            return value.decode("utf-8", errors="replace") if isinstance(value, bytes) else value or ""

        code, stdout, stderr, status = (
            None, text(error.stdout), text(error.stderr) + "\n" + str(error), "error"
        )
    except OSError as error:
        code, stdout, stderr, status = None, "", str(error), "error"
    (output / (name + ".json")).write_text(stdout, encoding="utf-8")
    (output / (name + ".stderr.txt")).write_text(stderr, encoding="utf-8")
    return {"name": name, "command": command, "returncode": code, "status": status,
            "report": name + ".json", "stderr": name + ".stderr.txt"}


def fingerprints(paths):
    return {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in paths}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/dependency-audit")
    args = parser.parse_args(argv)
    output = args.output.resolve()
    # Never overwrite a lockfile by choosing its containing directory as output.
    if output == ROOT or output == ROOT / "ui":
        parser.error("choose a dedicated report directory, for example target/dependency-audit")
    output.mkdir(parents=True, exist_ok=True)
    external_locks = sorted((ROOT / "external").glob("*/Cargo.lock"))
    paths = [ROOT / "Cargo.lock", ROOT / "ui/package-lock.json", *external_locks]
    if any(output == path.parent for path in paths):
        parser.error("audit reports must not use a package root as output")
    npm_scopes = [("ui", ROOT / "ui")]
    before = fingerprints(paths)
    cargo = shutil.which("cargo") or "cargo"
    npm = shutil.which("npm.cmd") or shutil.which("npm") or "npm"
    results = [run_audit("cargo", "cargo", [cargo, "audit", "--file", str(ROOT / "Cargo.lock"),
               "--db", str(output / "advisory-db"), "--json"], output)]
    for lock in external_locks:
        results.append(run_audit("cargo-" + lock.parent.name, "cargo", [cargo, "audit", "--file", str(lock),
                       "--db", str(output / "advisory-db"), "--json"], output))
    for name, directory in npm_scopes:
        results.append(run_audit(name, "npm", [npm, "audit", "--prefix", str(directory),
                       "--audit-level=high", "--json", "--cache", str(output / "npm-cache")], output))
    unchanged = before == fingerprints(paths)
    summary = {"checked_at": datetime.now(timezone.utc).isoformat(),
               "policy": {"cargo": "vulnerabilities fail; warnings retained",
                          "npm": "high/critical fail; low/moderate retained"},
               "lockfiles_unchanged": unchanged, "lockfile_sha256": before, "audits": results}
    (output / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n",
                                         encoding="utf-8")
    for result in results:
        print(result["name"] + ": " + result["status"])
    print("Reports: " + str(output))
    if not unchanged or any(result["status"] == "error" for result in results):
        return 2
    return 1 if any(result["status"] == "findings" for result in results) else 0


if __name__ == "__main__":
    raise SystemExit(main())
