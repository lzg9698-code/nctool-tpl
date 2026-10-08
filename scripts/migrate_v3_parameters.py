#!/usr/bin/env python3
"""Build a portable V3 bundle and a frozen Python oracle. Never write to V3.

Python/Jinja2/PyYAML are development tools only; product generation uses Rust.
No part dimensions, transient inputs or historical program text become defaults.
"""
from __future__ import annotations
import argparse, ast, copy, hashlib, json, math, re, shutil, sys
from pathlib import Path
import jinja2
from jinja2 import meta
import yaml

ROOT = Path(__file__).resolve().parents[1]
FORMAT = re.compile(r'(["\'])%0?\.(\d+)f\1\s*\|\s*format\(([^\n]*?)\)')
TRIM = re.compile(r'\((["\'])%\.2f\1\s*\|\s*format\(sync_feed\)\)\.rstrip\("0"\)\.rstrip\("\."\)')
OPS = {"keyway/keyway_rough.j2":"keyway.rough", "keyway/keyway_finish.j2":"keyway.finish", "keyway/keyway_chamfer.j2":"keyway.chamfer", "sync_turning_left_template.j2":"sync.left", "sync_turning_right_template.j2":"sync.right", "undercut_template.j2":"groove", "circlip_groove_template.j2":"circlip"}
STRINGS = {"U_RT","U_RTB","U_FT","U_FTB","U_CT","U_CTB","U_ID","U_FX"}
UNITS = {"U_A":"mm","U_B":"mm","U_C":"mm","U_D":"mm","U_Z":"mm","U_Q":"mm","U_RTD":"mm","U_FTD":"mm","U_RC":"mm","U_ALBR":"mm","U_ALBHF":"mm","U_ANG":"deg","U_RTRPM":"rpm","U_FTRPM":"rpm","U_RTRF":"mm/min","U_RTFF":"mm/min","U_FTF":"mm/min"}
def encoded(value): return json.dumps(value,ensure_ascii=False,sort_keys=True,separators=(",",":"),allow_nan=False).encode()
def sha(data): return hashlib.sha256(data).hexdigest()
def fingerprint(value):
    h=0xcbf29ce484222325
    for b in encoded(value): h=((h^b)*0x100000001b3)&0xffffffffffffffff
    return f"fnv1a64:{h:016x}"
def identifier(name): return "v3-"+re.sub(r"[^a-z0-9.-]+","-",name.removesuffix(".j2").lower()).strip("-")
def write(path,value): path.parent.mkdir(parents=True,exist_ok=True);path.write_text(json.dumps(value,ensure_ascii=False,indent=2)+"\n",encoding="utf-8")
def schema(fields,strings=()): return {"type":"object","properties":{f:{"type":"string" if f in strings else "number","title":f} for f in fields},"required":list(fields),"additionalProperties":False}
def input_schema(kind):
    if kind.startswith("keyway."):
        g=schema(["length_a","width_b","depth_c","shaft_d","position_z","corner_radius_q","form","param_id"],["form","param_id"]);g["properties"]["form"]["enum"]=["闭口","左开口","右开口"]
        t=schema(["tool_num","tool_offset","diameter","teeth","cutting_speed","feed_per_tooth"],["tool_num","tool_offset"]);t["properties"]["teeth"]["type"]="integer";t["required"]=["tool_num","tool_offset","diameter"]
        c=schema(["tool_num","tool_offset","depth","spindle_speed","feed","tool_diameter","plunge_depth","chisel_edge_width","protrusion"],["tool_num","tool_offset"])
        return {"type":"object","properties":{"geometry":g,"rough_tool":t,"finish_tool":t,"process":schema(["max_depth_per_pass","ramp_angle","rough_margin","semi_finish_margin","semi_finish_feed","corner_feed_factor"]),"chamfer":c,"enable_chamfer":{"type":"boolean"}},"required":["geometry","rough_tool","finish_tool","process","enable_chamfer"],"additionalProperties":False}
    if kind.startswith("sync."):
        s=schema(["sta_x","sta_z","end_x","end_z","ap_1","ap_2","sync_feed","current_sync_num"]);s["properties"]["current_sync_num"]["type"]="integer";return s
    if kind=="groove":
        s=schema(["gtype","side","std_key","raw_diameter","safety_dis"],["gtype","side","std_key"])
        s["properties"].update({f:{"type":"number"} for f in ["ES_d1","ES_z","ES_d2","ES_c","ES_c_ang","FS_d1","FS_z"]});s["properties"]["gtype"]["enum"]=["ES","FS"];s["properties"]["side"]["enum"]=["Left","Right"];return s
    if kind=="circlip":
        s=schema(["X_RAW_D","X_TIP_D","X_DOWN_D","WIDTH_CIRCLIP","MAX_CUTDIPTH","WIDTH_CUTER","FEEDRATE","z_positions"],["z_positions"])
        for f in s["properties"]:
            if f!="z_positions":s["properties"][f]["type"]=["string","number"]
        s["properties"]["z_positions"]["format"]="multiline";return s
    raise ValueError(kind)
def leaves(s,prefix=""):
    if s.get("type")=="object":
        for key,field in s["properties"].items():yield from leaves(field,prefix+"/"+key)
    else:yield prefix,s
def environment(source):
    env=jinja2.Environment(loader=jinja2.FileSystemLoader(source/"templates"),undefined=jinja2.StrictUndefined,trim_blocks=True,lstrip_blocks=True)
    env.globals.update(sin=lambda x:math.sin(math.radians(x)),cos=lambda x:math.cos(math.radians(x)),tan=lambda x:math.tan(math.radians(x)),sqrt=math.sqrt,abs=abs,round=round,pi=math.pi,radians=math.radians,degrees=math.degrees)
    env.filters.update(fmt_coord=lambda v:f"{float(v):+.3f}",fmt_val=lambda v:f"{float(v):.3f}")
    return env
def rewrite(text,ids):
    changes=[]
    text,n=TRIM.subn('(sync_feed | nc_trim_fixed(2))',text)
    if n:changes.append({"rule":"fixed_then_trim","count":n})
    prefix=re.compile(r'(["\'])%0?\.(\d+)f\1\s*\|\s*format\(')
    count=0
    while match:=prefix.search(text):
        start=match.end();end=start;depth=1
        while depth and end<len(text):
            if text[end]=='(':depth+=1
            elif text[end]==')':depth-=1
            end+=1
        if depth:raise ValueError('Unbalanced format expression')
        text=text[:match.start()]+f'({text[start:end-1]}) | v3_fixed({match[2]})'+text[end:];count+=1
    n=count
    if n:changes.append({"rule":"python_percent_format","count":n})
    for old,new in [(r'\|\s*fmt_coord\b','| v3_signed(3)'),(r'\|\s*fmt_val\b','| v3_fixed(3)'),(r'\btan\(([^()]*)\)',r'(\1 | tan_d)'),(r'\bsqrt\(([^()]*)\)',r'(\1 | sqrt)')]:
        text,n=re.subn(old,new,text)
        if n:changes.append({"rule":old,"count":n})
    # MiniJinja round is half-away; these V3 calls expect Python round.
    text,n=re.subn(r'\|\s*round\(0\)', '| nc_fixed(0) | int',text)
    if n:changes.append({"rule":"round_zero_to_even","count":n})
    for old,new in ids.items():text=text.replace('"'+old+'"','"'+new+'"').replace("'"+old+"'","'"+new+"'")
    if "tip_depth_map.get(tip_model, 29.61)" in text:
        changes.append({"rule":"dictionary_get_to_index_default","count":text.count("tip_depth_map.get(tip_model, 29.61)")})
        text=text.replace("tip_depth_map.get(tip_model, 29.61)","tip_depth_map[tip_model] | default(29.61)")
    return text,changes
def keyway_input():
    return {"geometry":{"length_a":100.0,"width_b":20.0,"depth_c":13.1,"shaft_d":60.0,"position_z":50.0,"corner_radius_q":8.0,"form":"闭口","param_id":"[100]"},"rough_tool":{"tool_num":"T15","tool_offset":"D1","diameter":12.0,"teeth":4,"cutting_speed":100.0,"feed_per_tooth":0.05},"finish_tool":{"tool_num":"T16","tool_offset":"D1","diameter":8.0,"teeth":4,"cutting_speed":100.0,"feed_per_tooth":0.04},"process":{"max_depth_per_pass":5.0,"ramp_angle":45.0,"rough_margin":0.3,"semi_finish_margin":0.1,"semi_finish_feed":0.0,"corner_feed_factor":0.5},"chamfer":{"tool_num":"T20","tool_offset":"D1","depth":0.5,"spindle_speed":4000,"feed":1200,"tool_diameter":8.0,"plunge_depth":2.0,"chisel_edge_width":0.0,"protrusion":40.0},"enable_chamfer":True}
class Capture:
    def __init__(self,env):self.env=env;self.contexts=[]
    def render_template(self,name,values):self.contexts.append(copy.deepcopy(values));return self.env.get_template(name).render(values)
def oracle(kind,inputs,env):
    parser=Capture(env)
    if kind.startswith("keyway."):
        from core.keyway_logic import KeywayGenerator
        from core.models import KeywayConfig,KeywayGeometry,KeywayTool,KeywayProcess,KeywayChamfer
        c=KeywayConfig(KeywayGeometry(**inputs["geometry"]),KeywayTool(**inputs["rough_tool"]),KeywayTool(**inputs["finish_tool"]),KeywayProcess(**inputs["process"]),KeywayChamfer(**inputs["chamfer"]),inputs["enable_chamfer"])
        r=KeywayGenerator(parser).generate(c);index={"keyway.rough":0,"keyway.finish":1,"keyway.chamfer":2}[kind];return {"contexts":[parser.contexts[index]],"texts":[[r.rough_gcode,r.finish_gcode,r.chamfer_gcode][index]]}
    if kind.startswith("sync."):
        from core.sync_turning import LeftSideGenerator,RightSideGenerator
        r=(LeftSideGenerator if kind=="sync.left" else RightSideGenerator)(parser).generate(**inputs);return {"contexts":parser.contexts,"texts":list(r[:2]),"next_sync_num":r[2]}
    if kind=="groove":
        from core.undercut_logic import GrooveGenerator
        text=GrooveGenerator(parser).generate(**inputs)
    else:
        from core.circlip_groove_logic import CirclipGrooveGenerator
        raw=inputs.copy();positions=raw.pop("z_positions");text=CirclipGrooveGenerator(parser).generate(raw,positions)
    return {"contexts":parser.contexts,"texts":[text]}
def build(source,out,frozen):
    # Freeze verification code, config and templates; never call source-project tests that write snapshots.
    frozen.mkdir(parents=True,exist_ok=True)
    for folder in ["core","configs","templates"]:
        shutil.copytree(source/folder,frozen/folder,dirs_exist_ok=True,ignore=shutil.ignore_patterns("__pycache__","*.pyc"))
    sys.path.insert(0,str(frozen.resolve()))
    env=environment(frozen);repo=json.loads((frozen/"configs/variable_repo.json").read_text(encoding="utf-8"))
    catalog=yaml.safe_load((frozen/"configs/templates.yaml").read_text(encoding="utf-8"))["templates"]
    paths=sorted((frozen/"templates").rglob("*.j2"));ids={p.relative_to(frozen/"templates").as_posix():identifier(p.relative_to(frozen/"templates").as_posix()) for p in paths}
    records=[];definitions={};mappings=[];fixtures=[];excluded=[]
    for p in paths:
        name=p.relative_to(frozen/"templates").as_posix();old=p.read_text(encoding="utf-8");new,changes=rewrite(old,ids);kind=OPS.get(name,"direct");fragment=Path(name).name.startswith("_keyway_")
        variables=sorted(meta.find_undeclared_variables(env.parse(old)))
        if kind=="direct":
            raw={}
            for v in variables:
                d=repo.get(v,{})
                value=d.get("default_value")
                # All historical outputs are excluded from shared definitions and fixture raw inputs.
                if isinstance(value,str) and ("\n" in value or v.startswith("SYT") or d.get("bound_area","") not in ["","无绑定"]):excluded.append(v);continue
                if value is None: value="T15" if v in STRINGS else 1.0
                if v not in STRINGS and isinstance(value,str):
                    try:value=float(value)
                    except ValueError:pass
                raw[v]=value
            s=schema(variables,[v for v in variables if isinstance(raw.get(v),str) or v not in raw])
        else:s=input_schema(kind);raw=None
        bindings={}
        for pointer,field in leaves(s):
            field=copy.deepcopy(field);field.pop("title",None);pid="v3-param-"+re.sub(r"[^a-z0-9]+","-",(pointer[1:] if kind=="direct" else kind.split('.')[0]+pointer).lower()).strip("-")
            unit=UNITS.get(pointer[1:]) if kind=="direct" else ("mm" if pointer.endswith(("diameter","length_a","width_b","depth_c","shaft_d","position_z","corner_radius_q","max_depth_per_pass")) else None)
            definition={"format_version":1,"id":pid,"title":repo.get(pointer[1:],{}).get("description",pointer[1:]),"schema":field,"group":"V3 "+kind.split('.')[0],"description":"V3 原始业务输入；无共享零件尺寸默认值"}
            if unit:definition["unit"]=unit
            definitions[pid]=definition;bindings[pointer]={"id":pid,**({"unit":unit} if unit else {})}
        md={"execution":{"action":"nc.generate.managed"},"nc":{"operation":{"id":kind,"version":1},"options":{"add_header_comment":False}},"parameters":{"bindings":bindings},"migration":{"source_path":name,"source_sha256":sha(p.read_bytes()),"syntax_changes":changes,"review_status":"machine_review_required"},"output_filename":catalog.get(name,{}).get("output_filename") or Path(name).stem}
        md["title"]=catalog.get(name,{}).get("name",name)
        md["nc"]["render_options"]={"trim_blocks":True,"lstrip_blocks":True}
        if kind in ("sync.left", "sync.right", "groove"):
            md["nc"]["trajectory_plan_version"] = 1
            md["nc"]["legacy_source"] = new
            md["nc"]["context_schema"] = {"type":"object","properties":{"trajectory_program":{"type":"string"}},"required":["trajectory_program"]}
            new = "{{ trajectory_program }}"
            changes.append({"rule":"native_trajectory_plan_v1","count":1})
        ext=catalog.get(name,{}).get("output_extension",".SPF");md["output_filename"]+=ext
        if fragment:md["fragment"]=True
        if kind.startswith("keyway."):
            md["parameters"]["tool_bindings"]={f'/{slot}_tool/{field}':{"slot":slot,"field":field,"kind":"attribute"} for slot in ["rough","finish"] for field in ["tool_num","tool_offset","diameter"]}
            for slot in ["rough","finish"]:md["parameters"]["tool_bindings"][f'/{slot}_tool/teeth']={"slot":slot,"field":"teeth","kind":"attribute","optional":True}
        elif kind=="direct" and name.startswith("INDEX G420/UZ_"):
            links={}
            for slot,prefix in [("rough","U_R"),("finish","U_F")]:
                fields={prefix+"T":("attribute","tool_num"),prefix+"TD":("attribute","diameter"),prefix+"TRPM":("value","rpm"),prefix+("TRF" if slot=="rough" else "TF"):("value","feed")}
                if slot=="rough":fields["U_RTFF"]=("value","semi_finish_feed")
                for field,(mode,target) in fields.items():
                    if field in variables:links["/"+field]={"slot":slot,"field":target,"kind":mode}
            if links:md["parameters"]["tool_bindings"]=links
        result_inputs=[v for v in variables if v not in (raw or {})] if kind=="direct" else []
        if result_inputs and not fragment:
            md["nc"]["result_inputs"]=["/"+v for v in result_inputs]
            md["nc"]["context_schema"]=copy.deepcopy(s)
            md["nc"]["result_input_domains"]={v:repo.get(v,{}).get("bound_area") or repo.get(v,{}).get("description", "current user operation") for v in result_inputs}
            for v in result_inputs:s["properties"].pop(v,None);bindings.pop("/"+v,None)
            s["required"]=[v for v in s["required"] if v not in result_inputs]
        md["nc"]["operation"]["input_bindings"]={path:path for path in bindings}
        asset={"source":new,"schema":s,"defaults":{},"metadata":md,"tags":["V3迁移",kind,"公共片段" if fragment else "加工程序"],"title":catalog.get(name,{}).get("name",name)}
        records.append({"group":"templates","name":ids[name],"asset":asset})
        mappings.append({"source":name,"id":ids[name],"processor":kind,"fragment":fragment,"raw_bindings":bindings,"variables":variables,"result_inputs":result_inputs,"syntax_changes":changes,"source_sha256":sha(p.read_bytes())})
        if kind in ("sync.left", "sync.right", "groove"):
            mappings[-1].update(trajectory_plan_version=1, trajectory_context_variables=["trajectory_program"])
        if kind=="direct" and not fragment:fixtures.append({"name":name,"id":ids[name],"operation":kind,"inputs":raw,"result_inputs":result_inputs})
    # Source tool associations separate facts from empirical cutting values. Missing facts stay explicit.
    nc=[]
    def add(group,id,asset):nc.append({"group":group,"name":id,"asset":{"format_version":1,"id":id,**asset}})
    add("tool_types","v3-milling",{"title":"V3 铣刀","operation":"milling"})
    add("materials","v3-unspecified",{"title":"V3 未记录材料（待补）","review_status":"unreviewed","missing_attributes":["material_grade"]})
    for field,prefix,diam,rpm,feed in [("U_RT","rough","U_RTD","U_RTRPM","U_RTRF"),("U_FT","finish","U_FTD","U_FTRPM","U_FTF")]:
        for tool,values in repo[field].get("associations",{}).items():
            tid="v3-tool-"+tool.lower();pid="v3-cutting-"+prefix+"-"+tool.lower()
            add("tools",tid,{"title":tool,"type_id":"v3-milling","attributes":{"tool_num":tool,"diameter":values[diam]},"missing_attributes":["teeth","tool_offset"],"source":"configs/variable_repo.json / "+field+" / "+tool})
            machining={"rpm":values[rpm],"feed":values[feed]}
            if "U_RTFF" in values:machining["semi_finish_feed"]=values["U_RTFF"]
            add("profiles",pid,{"title":"V3 "+prefix+" "+tool,"tool_id":tid,"material_id":"v3-unspecified","operation":"milling","mode":"fixed","feed_mode":"per_minute","values":machining,"review_status":"unreviewed"})
    standards=json.loads((ROOT/"plugins/nc/src/operation_standards.json").read_text(encoding="utf-8"));nc.append({"group":"standards","name":standards["id"],"asset":standards})
    collections={"template":{"version":1,"records":records},"parameters":{"version":1,"records":[{"group":"definitions","name":id,"asset":asset} for id,asset in sorted(definitions.items())]},"nc-parameters":{"version":1,"records":nc}}
    bundle={"format":"nctool-workspace","version":1,"collections":collections,"manifest":[{"collection":key,"provider":{"template":"template","parameters":"parameters","nc-parameters":"nc-parameters"}[key],"version":"2.0.0","fingerprint":fingerprint(value)} for key,value in collections.items()]}
    from v3_calculation_map import enrich
    write(out/"workspace.json",bundle);write(out/"mapping.json",enrich(mappings,frozen));write(out/"classification.json",{"excluded_historical_outputs":sorted(set(excluded)),"defaults_policy":"No part dimensions or historical output in shared defaults","missing_facts":"See tools.missing_attributes; no inferred teeth, offsets or material"})
    cases=[]
    for form,q,diam in [("闭口",8.,12.),("闭口",8.,8.),("闭口",10.,12.),("左开口",8.,12.),("右开口",8.,12.)]:
        c=keyway_input();c["geometry"].update(form=form,corner_radius_q=q);c["rough_tool"]["diameter"]=diam
        for name,kind in OPS.items():
            if kind.startswith("keyway."):cases.append({"name":name,"id":ids[name],"operation":kind,"inputs":copy.deepcopy(c)})
    for side in ["left","right"]:
        for start in [60.0,61.0,60.000000005,50.0]:cases.append({"name":f"sync_turning_{side}_template.j2","id":ids[f"sync_turning_{side}_template.j2"],"operation":"sync."+side,"inputs":{"sta_x":start,"sta_z":0.0,"end_x":50.0,"end_z":-10.0,"ap_1":2.0,"ap_2":3.0,"sync_feed":0.2,"current_sync_num":10}})
    for depth, step in [(5.0,5.0),(5.001,5.0),(0.001,5.0),(13.1,2.0)]:
        c=keyway_input();c["geometry"]["depth_c"]=depth;c["process"]["max_depth_per_pass"]=step
        name=next(name for name,kind in OPS.items() if kind=="keyway.rough")
        cases.append({"name":name,"id":ids[name],"operation":"keyway.rough","inputs":c})
    for name,kind in OPS.items():
        if kind in ["keyway.rough","keyway.finish"]:
            c=keyway_input();c["enable_chamfer"]=False
            cases.append({"name":name,"id":ids[name],"operation":kind,"inputs":c})
    from core.undercut_logic import ES_GROOVE_DB,FS_GROOVE_DB
    for gt,table in [("ES",ES_GROOVE_DB),("FS",FS_GROOVE_DB)]:
        for std in table:
            for side in ["Left","Right"]:
                v={"gtype":gt,"side":side,"std_key":std,"raw_diameter":100.0,"safety_dis":2.0}
                v.update({"ES_d1":80.,"ES_z":30.,"ES_d2":85.,"ES_c":0.5,"ES_c_ang":45.} if gt=="ES" else {"FS_d1":80.,"FS_z":30.})
                cases.append({"name":"undercut_template.j2","id":ids["undercut_template.j2"],"operation":"groove","inputs":v})
    for positions in ["10+2,20-1\n-2**2","2.6755,-1.2345,0","0x10,0o10,0b10,1_0.2_5"]:
        cases.append({"name":"circlip_groove_template.j2","id":ids["circlip_groove_template.j2"],"operation":"circlip","inputs":{"X_RAW_D":"100","X_TIP_D":"90+5","X_DOWN_D":"90","WIDTH_CIRCLIP":"4","MAX_CUTDIPTH":"2","WIDTH_CUTER":"3","FEEDRATE":"0.3","z_positions":positions}})
    for case in cases:case["expected"]=oracle(case["operation"],case["inputs"],env)
    write(out/"oracle.json",cases);write(out/"direct-inputs.json",fixtures)
    for filename,kind in [("keyway-raw","keyway.rough"),("sync-raw","sync.right"),("circlip-raw","circlip")]:
        case=next(c for c in cases if c["operation"]==kind)
        write(out/"requests"/(filename+".json"),{"template":case["id"],"operation_inputs":case["inputs"]})
    sync_case=next(c for c in cases if c["operation"]=="sync.right")
    write(out/"requests/process-dual.json",{"ops":[{"id":"turn","template":sync_case["id"],"operation_inputs":sync_case["inputs"]}],"outputs":[{"name":f"channel{i}.SPF","segments":[{"op":"turn","channel":i}]} for i in [1,2]]})
    hashes={p.relative_to(frozen).as_posix():sha(p.read_bytes()) for p in frozen.rglob("*") if p.is_file() and "__pycache__" not in p.parts}
    write(out/"source-hashes.json",hashes)
    print(json.dumps({"templates":len(records),"definitions":len(definitions),"oracle_cases":len(cases),"bundle":str(out/"workspace.json")},ensure_ascii=False))
if __name__=="__main__":
    parser=argparse.ArgumentParser();parser.add_argument("--source",type=Path,required=True);parser.add_argument("--output",type=Path,default=ROOT/"examples/v3-managed");parser.add_argument("--frozen",type=Path,default=ROOT/"target/parameters-validation/frozen-v3");args=parser.parse_args();build(args.source,args.output,args.frozen)
