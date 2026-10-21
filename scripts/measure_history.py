#!/usr/bin/env python3
"""Independent, reproducible measurement for the workspace history read path.

Builds isolated fixtures for the workspace history index (``_index/*.json``) of a
given size, drives the ignored Rust harness in ``cli/tests/history_measure.rs``
against each fixture, and records the per-operation medians as JSON under
``target/agent-preview/optimization/``.

This script never starts the WebUI/HTTP server, never touches a user workspace and
never writes into version-controlled files. Fixtures live under the Git-ignored
``target/agent-preview/optimization/fixtures`` tree.

Usage:
    python scripts/measure_history.py --label before
    python scripts/measure_history.py --label after --profile release
    python scripts/measure_history.py --compare before after
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import statistics
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = REPO_ROOT / "target" / "agent-preview" / "optimization"
FIXTURE_DIR = OUT_DIR / "fixtures"
TEST_NAME = "measure_history_read_query"
METRIC_PREFIX = "HISTORY_METRIC_JSON "

OPERATIONS = ("list", "query_default", "query_status", "query_text", "query_page")


def write_fixture(root: Path, size: int) -> Path:
    """Create a deterministic history fixture of ``size`` index summaries.

    Every entry is ~ a real ``save_summary`` record. ``alpha`` appears in ~1% of
    titles so the selective text query has a stable, size-proportional match set.
    """
    workspace = root / "workspace"
    index = workspace / ".nctool" / "runs" / "_index"
    (root / "home").mkdir(parents=True, exist_ok=True)
    index.mkdir(parents=True, exist_ok=True)
    for i in range(size):
        record = {
            "id": f"run-{i:06d}",
            "title": f"alpha batch {i}" if i % 100 == 0 else f"Job {i}",
            "action": "template.render" if i % 2 == 0 else "external.run",
            "status": "succeeded" if i % 3 == 0 else "failed",
            "started_at": size - i,
            "finished_at": None,
            "diagnostic_count": 0,
        }
        (index / f"run-{i:06d}.json").write_text(
            json.dumps(record, ensure_ascii=False), encoding="utf-8"
        )
    return workspace


def git_revision() -> str:
    try:
        return subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


def run_size(size: int, iterations: int, profile: str) -> dict:
    root = FIXTURE_DIR / f"size-{size}"
    if root.exists():
        for child in root.rglob("*"):
            if child.is_file():
                child.unlink()
    workspace = write_fixture(root, size)

    env = dict(os.environ)
    env["NCTOOL_HISTORY_WORKSPACE"] = str(workspace)
    env["NCTOOL_HISTORY_HOME"] = str(root / "home")
    env["NCTOOL_HISTORY_SIZE"] = str(size)
    env["NCTOOL_HISTORY_ITERATIONS"] = str(iterations)
    env["NCTOOL_HISTORY_LABEL"] = "run"

    command = [
        "cargo",
        "test",
        "--locked",
        "--target-dir",
        str(OUT_DIR / "history-build"),
        "-p",
        "nctool-cli",
        "--test",
        "history_measure",
        TEST_NAME,
        "--",
        "--ignored",
        "--nocapture",
    ]
    if profile == "release":
        command.insert(2, "--release")
    print(f"[measure] size={size} profile={profile}: {' '.join(command)}", flush=True)
    started = time.time()
    completed = subprocess.run(
        command, cwd=REPO_ROOT, env=env, capture_output=True, text=True
    )
    if completed.returncode != 0:
        sys.stderr.write(completed.stdout)
        sys.stderr.write(completed.stderr)
        raise SystemExit(f"harness failed for size={size} (rc={completed.returncode})")

    metrics = []
    for line in completed.stdout.splitlines():
        if line.startswith(METRIC_PREFIX):
            metrics.append(json.loads(line[len(METRIC_PREFIX) :]))
    if len(metrics) != 1:
        raise SystemExit(f"expected one metric line for size={size}, got {len(metrics)}")
    metric = metrics[0]
    metric["label"] = None  # filled in by caller
    metric["profile"] = profile
    metric["wall_seconds"] = round(time.time() - started, 3)
    return metric


def measure(label: str, sizes: list[int], iterations: int, profile: str) -> dict:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    payload = {
        "label": label,
        "profile": profile,
        "iterations": iterations,
        "sizes": sizes,
        "git_revision": git_revision(),
        "rust": rust_version(),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "recorded_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "results": {},
    }
    for size in sizes:
        metric = run_size(size, iterations, profile)
        payload["results"][str(size)] = metric

    target = OUT_DIR / f"history-{label}.json"
    target.write_text(json.dumps(payload, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps(payload, indent=2, ensure_ascii=False))
    print(f"[measure] recorded {target}")
    return payload


def rust_version() -> str:
    try:
        return subprocess.run(
            ["rustc", "--version"], capture_output=True, text=True, check=True
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


def compare(labels: list[str]) -> None:
    if len(labels) != 2:
        raise SystemExit("--compare needs exactly two labels, e.g. before after")
    reports = {}
    for label in labels:
        path = OUT_DIR / f"history-{label}.json"
        if not path.exists():
            raise SystemExit(f"missing report {path}; measure it first")
        reports[label] = json.loads(path.read_text(encoding="utf-8"))

    before, after = (reports[labels[0]], reports[labels[1]])
    print(
        f"before={labels[0]} profile={before['profile']} "
        f"after={labels[1]} profile={after['profile']}"
    )
    header = f"{'size':>6}  {'op':<20} {'before ms':>10} {'after ms':>10} {'ratio':>7}"
    print(header)
    print("-" * len(header))
    for size in sorted(before["results"], key=int):
        b_res = before["results"][size]
        a_res = after["results"].get(size, {})
        b_ops = {**b_res.get("isolation", {}), **b_res.get("operations", {})}
        a_ops = {**a_res.get("isolation", {}), **a_res.get("operations", {})}
        for op in ("read_parse", "list_current", "list_proposed", "query_current",
                   "query_proposed", *OPERATIONS):
            b_ms = b_ops[op]["median_ns"] / 1e6
            a_ms = a_ops.get(op, {}).get("median_ns", float("nan")) / 1e6
            ratio = a_ms / b_ms if b_ms else float("nan")
            print(f"{size:>6}  {op:<20} {b_ms:>10.3f} {a_ms:>10.3f} {ratio:>6.2f}x")


def parse_sizes(raw: str) -> list[int]:
    sizes = [int(part) for part in raw.split(",") if part.strip()]
    if not sizes or any(size < 1 for size in sizes):
        raise SystemExit("--sizes must list positive fixture sizes")
    return sizes


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", help="label to record (e.g. before/after)")
    parser.add_argument(
        "--sizes", default="100,1000,5000", help="comma-separated fixture sizes"
    )
    parser.add_argument("--iterations", type=int, default=20)
    parser.add_argument("--profile", choices=("debug", "release"), default="debug")
    parser.add_argument(
        "--compare", nargs=2, metavar=("BEFORE", "AFTER"), help="compare two reports"
    )
    args = parser.parse_args()
    labels = args.compare or ([args.label] if args.label else [])
    if any(not label or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_" for c in label) for label in labels):
        parser.error("labels must contain only letters, digits, hyphens and underscores")
    if args.iterations < 1:
        parser.error("--iterations must be positive")

    if args.compare:
        compare(args.compare)
    elif args.label:
        measure(args.label, parse_sizes(args.sizes), args.iterations, args.profile)
    else:
        parser.error("pass --label to record or --compare to diff two reports")


if __name__ == "__main__":
    main()
