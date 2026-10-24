#!/usr/bin/env python3
"""Measure catalog, full snapshot inspection and include rendering at asset scale.

Only generated fixtures in TemporaryDirectory are used; no workspace argument is
accepted. Timings include a fresh CLI process, startup and JSON serialization.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import time


def invoke(binary, home, workspace, action, value):
    start=time.perf_counter()
    result=subprocess.run([str(binary),"--home",str(home),"--workspace",str(workspace),"run",action,"--input","-"],
                          input=json.dumps(value).encode(),capture_output=True,timeout=90,
                          creationflags=subprocess.CREATE_NO_WINDOW if os.name=="nt" else 0)
    elapsed=(time.perf_counter()-start)*1000
    if result.returncode:
        raise RuntimeError("asset measurement action failed: "+action)
    response=json.loads(result.stdout)
    return elapsed,len(result.stdout),response["data"]


def measure(binary,size,repetitions):
    with tempfile.TemporaryDirectory(prefix="nctool-assets-measure-") as directory:
        root=Path(directory)
        home=root/"home"
        templates=root/"workspace/templates"
        home.mkdir()
        templates.mkdir(parents=True)
        source_bytes=0
        for i in range(size):
            name="root" if i==0 else f"leaf-{i:04d}"
            source='{% include "leaf-0001" %}Root {{ n }}' if i==0 else ("Leaf {{ n }} | " if i==1 else "{# "+"fixture "*150+"#}\n")
            source_bytes+=len(source.encode())
            asset={"source":source,"schema":{"type":"object","properties":{"n":{"type":"integer"}}},
                   "defaults":{"n":1},"tags":["fixture"],"metadata":{"title":name,"description":"Asset-scale fixture"}}
            (templates/(name+".json")).write_text(json.dumps(asset),encoding="utf-8")
        disk_bytes=sum(p.stat().st_size for p in templates.glob("*.json"))
        operations={}
        for label,action,value in [("catalog","template.catalog",{}),("inspect_snapshot","template.inspect",{"template":"root","include_snapshot":True}),
                                   ("render_include","template.render",{"template":"root","context":{"n":42}})]:
            invoke(binary,home,templates.parent,action,value)
            samples=[invoke(binary,home,templates.parent,action,value) for _ in range(repetitions)]
            for _,_,data in samples:
                if label=="catalog" and len(data["items"])!=size:
                    raise RuntimeError("catalog fixture count mismatch")
                if label=="inspect_snapshot" and len(data["snapshot"]["templates"])!=size:
                    raise RuntimeError("snapshot fixture count mismatch")
                if label=="render_include" and data["text"]!="Leaf 42 | Root 42":
                    raise RuntimeError("include rendering mismatch")
            operations[label]={"elapsed_ms_median":round(statistics.median(s[0] for s in samples),3),
                               "output_json_bytes":samples[0][1],"samples":repetitions}
        return {"template_count":size,"source_bytes":source_bytes,"asset_bytes":disk_bytes,"operations":operations}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary",type=Path,required=True)
    parser.add_argument("--sizes",default="100,1000")
    parser.add_argument("--repetitions",type=int,default=5)
    parser.add_argument("--output",type=Path,required=True)
    args=parser.parse_args()
    try:
        sizes=[int(s) for s in args.sizes.split(",")]
    except ValueError:
        parser.error("sizes must be integers")
    if not sizes or any(not 2<=n<=1024 for n in sizes) or not 1<=args.repetitions<=25:
        parser.error("sizes must be 2..1024 and repetitions must be 1..25")
    binary=args.binary.resolve()
    root=Path(__file__).resolve().parents[1]
    report={"platform":platform.platform(),"binary":str(binary),"binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),
            "source_sha256":{name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in ("plugins/template/src/lib.rs","assets/src/lib.rs","Cargo.lock")},
            "scope":"fresh release CLI process; generated assets only; includes startup, IO, snapshot and serialization",
            "results":[measure(binary,n,args.repetitions) for n in sizes]}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,ensure_ascii=False,indent=2)+"\n",encoding="utf-8")
    print(args.output)


if __name__=="__main__":
    main()
