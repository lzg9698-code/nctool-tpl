#!/usr/bin/env python3
"""Measure isolated CLI rendering and static-resource compression; no user assets."""
import argparse
import ctypes
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import time


def peak_working_set(process):
    if os.name != "nt":
        return None
    from ctypes import wintypes

    class Counters(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
            (name, ctypes.c_size_t) for name in (
                "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
                "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
                "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage",
            )
        ]

    counters = Counters()
    counters.cb = ctypes.sizeof(counters)
    function = ctypes.WinDLL("psapi").GetProcessMemoryInfo
    function.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
    function.restype = wintypes.BOOL
    if not function(int(process._handle), ctypes.byref(counters), counters.cb):
        raise ctypes.WinError()
    return counters.PeakWorkingSetSize


def invoke(binary, home, workspace, source, context, extensions):
    payload = json.dumps({"source": source, "context": context, "extensions": extensions}).encode()
    started = time.perf_counter()
    process = subprocess.Popen(
        [str(binary), "--home", str(home), "--workspace", str(workspace),
         "run", "template.render", "--input", "-"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    # communicate closes stdin and drains stdout, avoiding deadlock on larger output.
    output, error = process.communicate(payload, timeout=60)
    elapsed = (time.perf_counter() - started) * 1000
    memory = peak_working_set(process)
    if process.returncode:
        raise RuntimeError(error.decode(errors="replace"))
    result = json.loads(output)
    # CLI run returns ActionResult, unlike the HTTP envelope.
    text = result.get("data", {}).get("text")
    if not isinstance(text, str):
        raise RuntimeError("render result has no text: " + output[:200].decode(errors="replace"))
    return elapsed, memory, text


def measure(binary, math, repetitions):
    with tempfile.TemporaryDirectory(prefix="nctool-p2-measure-") as folder:
        home, workspace = Path(folder) / "home", Path(folder) / "workspace"
        home.mkdir()
        workspace.mkdir()
        if math:
            (home / "config.json").write_text(json.dumps({
                "profile": "template", "enabled": ["math"],
            }), encoding="utf-8")
        cases = [
            ("small", "Hello {{ name }}", {"name": "NCtool"}, [], "Hello NCtool"),
            ("rows-1000", "{% for row in rows %}{{ row.id }}:{{ row.name }}\n{% endfor %}",
             {"rows": [{"id": n, "name": "item"} for n in range(1000)]}, [], None),
        ]
        if math:
            cases.append(("math-1000", "{% for n in rows %}{{ n | sqrt }}\n{% endfor %}",
                          {"rows": list(range(1, 1001))}, ["math"], None))
        measurements = []
        for name, source, context, extensions, expected in cases:
            # One unreported warm-up; every sample still starts a new CLI process.
            invoke(binary, home, workspace, source, context, extensions)
            samples = [invoke(binary, home, workspace, source, context, extensions)
                       for _ in range(repetitions)]
            if expected and any(sample[2] != expected for sample in samples):
                raise RuntimeError("unexpected rendered output")
            if name == "rows-1000":
                expected_rows = "".join(f"{n}:item\n" for n in range(1000))
                if any(sample[2] != expected_rows for sample in samples):
                    raise RuntimeError("unexpected row rendering")
            memory = [sample[1] for sample in samples if sample[1] is not None]
            measurements.append({
                "case": name, "input_json_bytes": len(json.dumps(context).encode()),
                "output_bytes": len(samples[0][2].encode()), "samples": repetitions,
                "elapsed_ms_median": round(statistics.median(x[0] for x in samples), 3),
                "elapsed_ms_min": round(min(x[0] for x in samples), 3),
                "peak_working_set_bytes_median": statistics.median(memory) if memory else None,
            })
        return {"binary": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "binary_bytes": binary.stat().st_size, "math": math, "measurements": measurements}


def compression(resources):
    files = [p for p in resources.rglob("*") if p.is_file() and p.suffix in (".js", ".css", ".html", ".svg")]
    start = time.perf_counter()
    pairs = [(p.read_bytes(), gzip.compress(p.read_bytes(), compresslevel=9, mtime=0)) for p in files]
    compress_ms = (time.perf_counter() - start) * 1000
    samples = []
    for _ in range(20):
        start = time.perf_counter()
        for raw, packed in pairs:
            if gzip.decompress(packed) != raw:
                raise RuntimeError("compression round trip mismatch")
        samples.append((time.perf_counter() - start) * 1000)
    return {"files": len(files), "raw_bytes": sum(len(x[0]) for x in pairs),
            "gzip_bytes": sum(len(x[1]) for x in pairs), "compression_ms": round(compress_ms, 3),
            "decompress_all_ms_median": round(statistics.median(samples), 3),
            "note": "Offline gzip round trip, not a Rust server implementation or final binary savings."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--default", type=Path, required=True)
    parser.add_argument("--math", type=Path, required=True)
    parser.add_argument("--resources", type=Path, default=Path("cli/ui"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repetitions", type=int, default=9)
    args = parser.parse_args()
    if not 1 <= args.repetitions <= 100:
        parser.error("repetitions must be 1..100")
    report = {"platform": platform.platform(), "python": platform.python_version(),
              "timing_scope": "fresh CLI process: startup + runtime boot + JSON input + render + serialization; not pure engine latency",
              "memory_scope": "Windows process peak working set, including startup and output; unsupported platforms report null",
              "builds": [measure(args.default.resolve(), False, args.repetitions),
                         measure(args.math.resolve(), True, args.repetitions)],
              "compression": compression(args.resources)}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(str(args.output))


if __name__ == "__main__":
    main()
