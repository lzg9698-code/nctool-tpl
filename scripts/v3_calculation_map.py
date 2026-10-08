"""Development-only trace of raw inputs, Python helpers and template-local rules."""
import ast
import re

KEYWAY = {
    "U_A":"geometry.length_a", "U_B":"geometry.width_b", "U_C":"geometry.depth_c",
    "U_D":"geometry.shaft_d", "U_Z":"geometry.position_z", "U_Q":"geometry.corner_radius_q",
    "U_ID":"geometry.param_id", "U_FX":"geometry.form", "is_left_open":"geometry.form == 左开口",
    "U_RT":"rough_tool.tool_num", "U_RTB":"rough_tool.tool_offset", "U_RTD":"rough_tool.diameter",
    "U_FT":"finish_tool.tool_num", "U_FTB":"finish_tool.tool_offset", "U_FTD":"finish_tool.diameter",
    "U_RC":"process.max_depth_per_pass", "U_ANG":"process.ramp_angle",
    "U_ALBR":"process.rough_margin", "U_ALBHF":"process.semi_finish_margin",
    "U_CT":"chamfer.tool_num", "U_CTB":"chamfer.tool_offset", "U_CHAMFER_DEPTH":"chamfer.depth",
    "R1":"chamfer.spindle_speed", "R2":"chamfer.feed", "R3":"chamfer.tool_diameter",
    "R4":"chamfer.plunge_depth", "R5":"chamfer.chisel_edge_width", "R6":"chamfer.protrusion",
    "rough_n":"round_even(1000*rough_tool.cutting_speed/(pi*rough_tool.diameter)) or explicitly selected slot rpm",
    "finish_n":"round_even(1000*finish_tool.cutting_speed/(pi*finish_tool.diameter)) or explicitly selected slot rpm",
    "rough_vf":"round_even(rough_n*rough_tool.teeth*rough_tool.feed_per_tooth) or selected slot feed",
    "finish_vf":"round_even(finish_n*finish_tool.teeth*finish_tool.feed_per_tooth) or selected slot feed",
    "rough_vf_corner":"round_even(rough_vf*process.corner_feed_factor)",
    "semi_vf":"round_even(process.semi_finish_feed if positive else finish_vf); template half-feed also uses round_even",
    "depths":"positive exact 0.001mm integer ticks: full passes plus remainder; at most 10000 layers",
    "target_depths":"cumulative depth ticks / 1000 (never floating-point cumulative additions)",
    "layer_count":"len(depths)",
    "path_type":"form first; round head abs(q-b/2)<0.001; rough two-pass when tool_radius>=q-margin-0.001, otherwise three-pass",
    "tool_num":"stage tool number", "tool_offset":"stage compensation number",
    "spindle_speed":"stage rpm", "next_tool_num":"rough -> finish; finish -> enabled chamfer; otherwise empty",
}
SYNC = {
    "cut_num":"ratio=(sta_x-end_x)/(ap_1+ap_2); near integer within 1e-8 and >0 uses nearest, otherwise ceil; max10000",
    "actual_sta_x":"end_x + cut_num*(ap_1+ap_2)",
    "actual_sta_z":"sta_z + ((actual_sta_x-sta_x)/2)/tan_deg(25)-2 (left channel uses correction)",
    "channel":"one frozen context per channel 1/2",
    "sync_num_start":"current_sync_num; advance by cut_num+1; no-cut does not advance",
    "warnings":"right-side thresholds ap_1>5, ap_2>8, sync_feed>0.5; do not clamp",
    "side":"processor sync.left/right",
}
GROOVE = {
    "STD_KEY":"std_key; lookup frozen V3 ES_GROOVE_DB/FS_GROOVE_DB",
    "groove_type":"gtype", "side":"side", "R":"standard.r",
    "X_START":"raw_diameter+2*safety_dis", "D1_CUT":"selected d1-2*standard.t1",
    "D1_COMP_START":"selected d1+2*safety_dis",
    "Z_START":"selected z +/- (standard.f+safety_dis/tan_deg(first standard angle)); right plus, left minus",
    "ANG_1":"right angle-180; left -angle (degrees)",
    "ES_Z":"ES_z", "ES_D2":"ES_d2", "D2_COMP_CUT":"ES_d2-2*ES_c*tan_deg(ES_c_ang)-0.5; positive result required",
    "Z_MID":"FS_z -/+ (standard.t2-standard.r+standard.r/tan_deg(45-standard.FS_ang_2/2)); right minus",
    "FS_Z_PLUS1":"FS_z+1", "FS_Z_MINUS1":"FS_z-1",
    "ANG_2":"right 90-standard.FS_ang_2; left 90+standard.FS_ang_2",
}
SOURCES = {
    "keyway":"core/keyway_logic.py", "sync":"core/sync_turning.py",
    "groove":"core/undercut_logic.py", "circlip":"core/circlip_groove_logic.py",
}

def enrich(mappings, frozen):
    for row in mappings:
        text=(frozen/"templates"/row["source"]).read_text(encoding="utf-8")
        assignments=[]
        for match in re.finditer(r"\{%[-+]?\s*set\s+(\w+)\s*=\s*(.*?)\s*[-+]?%\}",text,re.S):
            assignments.append({"target":match[1],"expression":match[2],"line":text.count('\n',0,match.start())+1})
        local={item["target"] for item in assignments}
        row["template_assignments"]=assignments
        row["template_dependencies"]=re.findall(r"\{%[-+]?\s*(?:include|import|extends)\s+[\"']([^\"']+)",text)
        group=row["processor"].split('.')[0]
        rules={"keyway":KEYWAY,"sync":SYNC,"groove":GROOVE}.get(group,{})
        variables={}
        for name in row["variables"]:
            if name in local:
                rule="Template-local calculation; see template_assignments and branch expressions"
                origin="template_local"
            elif group=="circlip":
                rule=("z_positions: comma/newline split; restricted numeric AST (256 chars,64 nodes,pow<=8,abs<=1e9), Python binary round3; fmt_coord signed3" if name=="z_offsets" else f"{name}: restricted numeric AST, Python binary round3, geometry checks; fmt_val fixed3")
                origin="processor"
            elif name in row["result_inputs"]:
                rule="Explicit result_bindings to a preceding operation from this task, optional channel; never historical variable text"
                origin="current_task"
            else:
                rule=rules.get(name,f"Raw /{name}; validate shared definition, template Schema and NC specs")
                origin="processor" if group in SOURCES else "raw"
            variables[name]={"origin":origin,"rule":rule}
        row["variable_rules"]=variables
        row["algorithm_version"]=1
        row["rust_processor"]="plugins/nc/src/operation.rs" if group in SOURCES else "plugins/nc/src/managed.rs"
        if group in SOURCES:
            source=SOURCES[group];tree=ast.parse((frozen/source).read_text(encoding="utf-8"))
            row["python_trace"]={"source":source,"functions":[{"name":node.name,"line":node.lineno} for node in ast.walk(tree) if isinstance(node,(ast.FunctionDef,ast.AsyncFunctionDef))],"support":["core/validation.py","core/render_snapshot.py","core/models.py","core/template_expression.py"]}
        if row["fragment"]:
            row["acceptance"]="Shared fragment: accepted through every dependent keyway stage, not standalone raw input"
    return mappings
