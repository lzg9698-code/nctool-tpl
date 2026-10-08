#!/usr/bin/env python3
"""CLI and HTTP raw-input acceptance against the frozen V3 oracle.

Allowed output adaptation is declared before comparison: LF line endings,
leading/trailing whitespace per line, and trailing newline only. No numeric,
instruction, channel, ordering or synchronization differences are ignored.
"""
from __future__ import annotations
import argparse, contextlib, copy, difflib, json, math, os, re, subprocess, sys, tempfile
from pathlib import Path
import urllib.request, urllib.error
from migrate_v3_parameters import encoded, identifier, write, sha
ROOT=Path(__file__).resolve().parents[1]
CLIENT=urllib.request.build_opener(urllib.request.ProxyHandler({}))
def norm(text):return "\n".join(line.strip() for line in text.replace("\r\n","\n").strip().split("\n"))
def compare(a,b,path=""):
    if isinstance(a,dict):
        for k,v in a.items():compare(v,b.get(k),path+"/"+k)
    elif isinstance(a,list):
        if not isinstance(b,list) or len(a)!=len(b):raise AssertionError(path+" length mismatch")
        for i,v in enumerate(a):compare(v,b[i],path+f"/{i}")
    elif isinstance(a,float):
        if not isinstance(b,(float,int)) or not math.isclose(a,b,rel_tol=2e-13,abs_tol=1e-11):raise AssertionError(f"{path}: {a} != {b}")
        if f"{a:.3f}"!=f"{b:.3f}":raise AssertionError(path+" output precision differs")
    elif a!=b:raise AssertionError(f"{path}: {a!r} != {b!r}")
def rpc(url,path,body=None):
    req=urllib.request.Request(url+"/api/v2/"+path,data=None if body is None else encoded(body),headers={"Content-Type":"application/json"})
    try:
        with CLIENT.open(req,timeout=60) as r:result=json.load(r)
    except urllib.error.HTTPError as e:result=json.load(e)
    if not result["ok"]:raise RuntimeError(json.dumps(result["error"],ensure_ascii=False))
    return result["data"]
@contextlib.contextmanager
def server(binary,base):
    p=subprocess.Popen([str(binary),"--home",str(base/"home"),"--workspace",str(base/"workspace"),"--profile","nc","ui","--port","0"],stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,text=True,encoding="utf-8")
    try:
        line=p.stderr.readline();m=re.search(r"http://127\.0\.0\.1:\d+",line)
        if not m:raise RuntimeError("server did not start: "+line+p.stderr.read())
        yield m[0]
    finally:p.terminate();p.wait(timeout=15)
def verify(binary,assets,report):
    report.mkdir(parents=True,exist_ok=True);passed=[];failures=[]
    bundle=json.loads((assets/"workspace.json").read_text(encoding="utf-8"));cases=json.loads((assets/"oracle.json").read_text(encoding="utf-8"));direct=json.loads((assets/"direct-inputs.json").read_text(encoding="utf-8"))
    with tempfile.TemporaryDirectory(prefix="managed-",dir=report) as temp:
        base=Path(temp);home=base/"home";workspace=base/"workspace"
        with server(binary,base) as url:
            rpc(url,"bundle/import",{"bundle":bundle})
            def http(action,input):return rpc(url,"actions/"+action,{"input":input})
            def cli(action,input):
                r=subprocess.run([str(binary),"--home",str(home),"--workspace",str(workspace),"--profile","nc","run",action,"--input","-"],input=encoded(input),capture_output=True)
                if r.returncode:raise RuntimeError(r.stderr.decode("utf-8"))
                return json.loads(r.stdout)
            for index,case in enumerate(cases):
                req={"template":case["id"],"operation_inputs":case["inputs"],"trim_blocks":True,"lstrip_blocks":True}
                try:
                    expected=case["expected"]
                    for entry in [cli,http]:
                        r=entry("nc.generate.managed",req);prepared=r["data"]["prepared"]
                        compare(expected["contexts"],prepared["contexts"])
                        if "next_sync_num" in expected:compare(expected["next_sync_num"],prepared["next_sync_num"])
                        if len(expected["texts"])!=len(r["artifacts"]):raise AssertionError("channel count")
                        for ch,(a,b) in enumerate(zip(expected["texts"],r["artifacts"])):
                            if norm(a)!=norm(b["text"]):
                                (report/f"diff-{index}-{ch}.txt").write_text("\n".join(difflib.unified_diff(norm(a).splitlines(),norm(b["text"]).splitlines(),fromfile="Python",tofile="Rust")),encoding="utf-8")
                                raise AssertionError(f"program mismatch, diff-{index}-{ch}.txt")
                    passed.append({"case":index,"template":case["id"],"calculation":True,"program":True,"cli":True,"http":True})
                except Exception as e:failures.append({"case":index,"template":case["id"],"error":str(e)})
            # Main programs receive current generated fragments, never historical text.
            primary={kind:next(c for c in cases if c["operation"]==kind) for kind in ["sync.right","sync.left","groove","circlip","keyway.rough","keyway.finish","keyway.chamfer"]}
            from migrate_v3_parameters import environment
            sys.path.insert(0,str(ROOT/"target/parameters-validation/frozen-v3"))
            env=environment(ROOT/"target/parameters-validation/frozen-v3")
            # Serial batches propagate the actual next sync number through both channels.
            for kind in ["sync.left","sync.right"]:
                from migrate_v3_parameters import oracle
                first=primary[kind];second=copy.deepcopy(first["inputs"]);second["sta_x"]=61.0;second["current_sync_num"]=first["expected"]["next_sync_num"]
                expected_second=oracle(kind,second,env);raw_second=copy.deepcopy(second);raw_second.pop("current_sync_num")
                request={"ops":[{"id":"first","template":first["id"],"operation_inputs":first["inputs"]},{"id":"second","template":first["id"],"operation_inputs":raw_second,"sync_from":"first"}],"outputs":[{"name":f"ch{ch+1}.SPF","segments":[{"op":"first","channel":ch+1},{"op":"second","channel":ch+1}]} for ch in range(2)]}
                try:
                    for entry in [cli,http]:
                        r=entry("process.generate.managed",request)
                        for ch in range(2):
                            expected=first["expected"]["texts"][ch].rstrip('\n')+'\n'+expected_second["texts"][ch]
                            if norm(expected)!=norm(r["artifacts"][ch]["text"]):raise AssertionError("batch channel or sync numbering differs")
                    passed.append({"test":"sync_batch","operation":kind,"cli":True,"http":True,"program":True})
                except Exception as e:failures.append({"test":"sync_batch","operation":kind,"error":str(e)})
            for case in direct:
                try:
                    inputs=case["inputs"].copy();ops=[];bindings={}
                    for i,field in enumerate(case["result_inputs"]):
                        kind="sync.left" if "LEFT" in field else "sync.right"
                        if "RELIEF_GROOVE" in field:kind="groove"
                        if "SHARP_GROOVE" in field:kind="circlip"
                        opid=f"fragment-{i}";channel=2 if field.endswith(("CH2","B","b")) else 1
                        if field.startswith("FINISH_TURNING") or field=="left_T_START3_END3":
                            # The old variable repository stores handwritten/historical paths.
                            # They are not calculators or defaults. This test explicitly binds
                            # a newly generated user operation with its own original inputs.
                            source="G1 X{{ diameter | v3_fixed(3) }} Z{{ end_z | v3_fixed(3) }} F{{ feed | v3_fixed(3) }}"
                            values={"diameter":50.0,"end_z":-10.0,"feed":0.2}
                            ops.append({"id":opid,"source":source,"operation_inputs":values})
                            bindings["/"+field]={"op":opid};inputs[field]="G1 X50.000 Z-10.000 F0.200"
                        else:
                            c=primary[kind];ops.append({"id":opid,"template":c["id"],"operation_inputs":c["inputs"],"trim_blocks":True,"lstrip_blocks":True})
                            if kind in ["groove","circlip"]:channel=1
                            bindings["/"+field]={"op":opid,"channel":channel}
                            inputs[field]=c["expected"]["texts"][channel-1].rstrip('\n')
                    expected=env.get_template(case["name"]).render(inputs)
                    request={"template":case["id"],"operation_inputs":case["inputs"],"trim_blocks":True,"lstrip_blocks":True}
                    if ops:case["managed_process"]={"ops":ops+[{"id":"main",**request,"result_bindings":bindings}],"outputs":[{"name":"main.MPF","segments":[{"op":"main"}]}]}
                    for entry in [cli,http]:
                        if ops:r=entry("process.generate.managed",case["managed_process"])
                        else:r=entry("nc.generate.managed",request)
                        if norm(expected)!=norm(r["artifacts"][0]["text"]):
                            path=report/(case["id"]+".diff");path.write_text("\n".join(difflib.unified_diff(norm(expected).splitlines(),norm(r["artifacts"][0]["text"]).splitlines())),encoding="utf-8");raise AssertionError(str(path))
                    passed.append({"template":case["id"],"program":True,"cli":True,"http":True,"composition":bool(ops)})
                except Exception as e:failures.append({"template":case["id"],"error":str(e)})
            # Later process failure must have no deliverable, even after a valid first op.
            good=cases[0];bad=next(c for c in cases if c["operation"]=="circlip");bad_input={**bad["inputs"],"z_positions":"1,__import__('os')"}
            try:http("process.generate.managed",{"ops":[{"template":good["id"],"operation_inputs":good["inputs"]},{"template":bad["id"],"operation_inputs":bad_input}]});failures.append({"test":"atomic_failure","error":"invalid expression accepted"})
            except RuntimeError:passed.append({"test":"atomic_failure","http":True})
            from migrate_v3_parameters import oracle
            invalid=[]
            for path,value in [("geometry/depth_c",0.0009),("process/max_depth_per_pass",0.0009),("process/max_depth_per_pass",0.001),("rough_tool/diameter",25),("process/ramp_angle",0)]:
                c=copy.deepcopy(primary["keyway.rough"]);group,field=path.split('/');c["inputs"][group][field]=value;invalid.append(c)
            for field,value in [("sync_feed",0.205),("end_x",100),("current_sync_num",1.5)]:
                c=copy.deepcopy(primary["sync.right"]);c["inputs"][field]=value;invalid.append(c)
            c=copy.deepcopy(primary["groove"]);c["inputs"]["std_key"]="missing";invalid.append(c)
            for expression in ["True","1/0","2**9","1e10","1_","01","1%2","1//2","(-1)**0.5",'+'.join(['1']*22),"__import__('os')"]:
                c=copy.deepcopy(primary["circlip"]);c["inputs"]["z_positions"]=expression;invalid.append(c)
            for index,c in enumerate(invalid):
                try:
                    try:oracle(c["operation"],c["inputs"],env)
                    except Exception:pass
                    else:raise AssertionError("Python accepted invalid fixture")
                    req={"template":c["id"],"operation_inputs":c["inputs"]}
                    for entry in [cli,http]:
                        try:entry("nc.generate.managed",req)
                        except RuntimeError:pass
                        else:raise AssertionError("Rust accepted invalid fixture")
                    passed.append({"invalid_case":index,"template":c["id"],"python_rejected":True,"cli_rejected":True,"http_rejected":True})
                except Exception as e:failures.append({"invalid_case":index,"error":str(e)})
    write(report/"report.json",{"platform":sys.platform,"binary":str(binary),"binary_sha256":sha(binary.read_bytes()),"allowed_adaptations":["LF","line outer whitespace","final newline"],"passed":passed,"failures":failures,"webui":"pending browser report"})
    write(assets/"direct-inputs.json",direct)
    main=next((c["managed_process"] for c in direct if c.get("managed_process")),None)
    if main:write(assets/"requests/main-current.json",main)
    print(json.dumps({"passed":len(passed),"failed":len(failures),"failures":failures[:8]},ensure_ascii=False));return 1 if failures else 0
if __name__=="__main__":
    import sys
    parser=argparse.ArgumentParser();parser.add_argument("--binary",type=Path,required=True);parser.add_argument("--assets",type=Path,default=ROOT/"examples/v3-managed");parser.add_argument("--report",type=Path,default=ROOT/"target/parameters-validation/comparison");a=parser.parse_args();sys.exit(verify(a.binary.resolve(),a.assets,a.report.resolve()))
