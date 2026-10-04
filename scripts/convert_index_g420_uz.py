#!/usr/bin/env python3
"""一次性转换用户选定的 UZ 模板并验证；不修改 V3、不注册内置模板。
需要 jinja2 和 PyYAML，只用于迁移，不是主程序的运行依赖。
"""

from __future__ import annotations
import argparse
import contextlib
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess
import tempfile
import urllib.error
import urllib.request
import jinja2
from jinja2 import meta
import yaml

FILES = [
    "UZ_DJ_X.j2",
    *[f"UZ_FKM_temp{i}.j2" for i in range(1, 5)],
    *[f"UZ_RKM_temp{i}.j2" for i in range(1, 8)],
]
FORMAT = re.compile(r"{{\s*([\"\'])%0?\.(\d+)f\1\s*\|\s*format\((.*?)\)\s*}}", re.S)
ZERO = re.compile(r"(?<![\d.])-0(?:\.0+)?(?![\d.])")
STRINGS = {"U_RT", "U_RTB", "U_FT", "U_FTB", "U_CT", "U_CTB", "U_ID", "U_FX"}
CLIENT = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def encoded(value: object) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    ).encode()


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fingerprint(value: object) -> str:
    result = 0xCBF29CE484222325
    for byte in encoded(value):
        result = ((result ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"fnv1a64:{result:016x}"


def identifier(name: str) -> str:
    return "index-g420-" + Path(name).stem.lower().replace("_", "-")


def rpc(url: str, route: str, body: dict | None = None) -> dict:
    request = urllib.request.Request(
        url + "/api/v2/" + route,
        data=None if body is None else encoded(body),
        headers={} if body is None else {"Content-Type": "application/json"},
    )
    try:
        with CLIENT.open(request, timeout=30) as response:
            result = json.load(response)
    except urllib.error.HTTPError as error:
        result = json.load(error)
    if not result["ok"]:
        raise RuntimeError(json.dumps(result["error"], ensure_ascii=False))
    return result["data"]


def action(url: str, name: str, value: dict) -> dict:
    return rpc(url, "actions/" + name, {"input": value})


@contextlib.contextmanager
def server(binary: Path):
    with tempfile.TemporaryDirectory(prefix="nctool-uz-verify-") as temp:
        p = subprocess.Popen(
            [
                str(binary.resolve()),
                "--home",
                str(Path(temp) / "home"),
                "--workspace",
                str(Path(temp) / "workspace"),
                "--profile",
                "nc",
                "ui",
                "--port",
                "0",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            line = p.stderr.readline()
            match = re.search(r"http://127\.0\.0\.1:\d+", line)
            if not match:
                raise RuntimeError("验证服务启动失败：" + line)
            yield match[0]
        finally:
            p.terminate()
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                p.kill()
                p.wait(timeout=5)
            p.stderr.close()


def write(path: Path, value: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        if path.read_bytes() != value:
            raise RuntimeError(f"已有产物不同，拒绝覆盖：{path}")
        return
    with path.open("xb") as stream:
        stream.write(value)


def convert(
    name: str, raw: bytes, repo: dict, cfg: dict, commit: str, root: Path
) -> tuple[dict, dict]:
    source = raw.decode("utf-8-sig").replace("\r\n", "\n").replace("\r", "\n")
    variables = sorted(
        meta.find_undeclared_variables(jinja2.Environment().parse(source)) - {"tan"}
    )
    changed, count = FORMAT.subn(
        lambda m: "{{ (" + m[3] + ") | nc_fixed(" + m[2] + ") }}", source
    )
    changed, tans = re.subn(r"\btan\(\s*U_ANG\s*\)", "(U_ANG | tan_d)", changed)
    if "|format(" in changed.replace(" ", "") or re.search(r"\btan\(", changed):
        raise RuntimeError("未处理扩展：" + name)
    analysis_env = jinja2.Environment()
    analysis_env.filters.update(
        {"nc_fixed": lambda value, decimals: value, "tan_d": lambda value: value}
    )
    if sorted(meta.find_undeclared_variables(analysis_env.parse(changed))) != variables:
        raise RuntimeError("变量改变：" + name)
    fields, specs, defaults, changes, missing = {}, {}, {}, [], {}
    for variable in variables:
        info = repo[variable]
        kind = (
            "string"
            if variable in STRINGS
            else "integer"
            if info.get("value_type") == "int" or info.get("data_type") == "int"
            else "number"
        )
        cast = str if kind == "string" else int if kind == "integer" else float
        desc = info.get("description", variable)
        field = {"type": kind, "title": desc, "description": variable}
        spec = {"name": variable, "kind": kind, "required": True, "description": desc}
        default = cast(info["default_value"])
        options = [cast(v) for v in info.get("options", [])]
        for controller in variables:
            table = [
                [key, cast(values[variable])]
                for key, values in repo[controller].get("associations", {}).items()
                if variable in values
            ]
            if table:
                spec["derive"] = {"from": controller, "table": table}
                field["description"] += f"；由 {controller} 联动，生成时以查表结果为准"
                for _, value in table:
                    if value not in options:
                        options.append(value)
                        changes.append(f"{variable} 补入关联表候选值 {value}")
                effective = [
                    value
                    for key, value in table
                    if key == str(repo[controller]["default_value"])
                ]
                if effective and default != effective[0]:
                    changes.append(
                        f"{variable} 默认值 {default} → {effective[0]}，按 {controller} 默认刀具关联表"
                    )
                    default = effective[0]
        if options:
            field["enum"] = options
            spec["options"] = options
        for old, new, target in [
            ("min_value", "minimum", "min"),
            ("max_value", "maximum", "max"),
        ]:
            if old in info:
                field[new] = info[old]
                spec[target] = info[old]
        unit = (
            "°"
            if variable == "U_ANG"
            else "rpm"
            if variable in ["R1", "U_FTRPM", "U_RTRPM"]
            else "mm/min"
            if variable in ["R2", "U_FTF", "U_RTRF", "U_RTFF"]
            else "mm"
            if kind != "string"
            else None
        )
        if unit:
            field["x-unit"] = unit
            spec["unit"] = unit
        field["default"] = default
        spec["default"] = default
        defaults[variable] = default
        fields[variable] = field
        specs[variable] = spec
        if info.get("associations"):
            absent = [
                str(v)
                for v in info.get("options", [])
                if str(v) not in info["associations"]
            ]
            if absent:
                missing[variable] = absent
    title = re.search(r"{#\s*NAME:\s*(.*?)\s*#}", source).group(1)
    config = cfg.get("INDEX G420/" + name, {})
    asset = {
        "source": changed,
        "tags": [
            "INDEX G420",
            "键槽",
            "V3 转换",
            "unreviewed",
            "倒角" if "DJ" in name else "精铣" if "FKM" in name else "粗铣",
        ],
        "schema": {"type": "object", "properties": fields, "required": variables},
        "defaults": defaults,
        "metadata": {
            "title": title,
            "description": f"{name}：V3 转换，保留原刀路；机床/工艺未审核。请从 G 代码生成页面使用。",
            "output_name": Path(name).stem + config.get("output_extension", ".SPF"),
            "review_status": "unreviewed",
            "domain": {"machine_family": "INDEX G420", "process": "keyway"},
            "nc": {
                "specs": list(specs.values()),
                "render_options": {"trim_blocks": True, "lstrip_blocks": True},
                "required_extensions": ["math", "nc"],
            },
            "v3": {
                "path": str(root / "templates/INDEX G420" / name),
                "sha256": sha(raw),
                "commit": commit,
                "parameters": {v: repo[v] for v in variables},
                "template_config": config,
                "license": "源资产许可保持不变；用户指定转换，不进入内置发行模板",
            },
        },
    }
    ledger = {
        "id": identifier(name),
        "source": name,
        "source_sha256": sha(raw),
        "converted_sha256": sha(changed.encode()),
        "parameter_count": len(variables),
        "printf_replacements": count,
        "degree_tangent_replacements": tans,
        "normalizations": changes,
        "unmapped_tool_options": missing,
        "review_status": "unreviewed",
    }
    return asset, ledger


def effective(asset: dict, overrides: dict) -> dict:
    result = {**asset["defaults"], **overrides}
    for spec in asset["metadata"]["nc"]["specs"]:
        if "derive" in spec:
            rule = spec["derive"]
            result[spec["name"]] = next(
                v for k, v in rule["table"] if k == result[rule["from"]]
            )
    return result


def verify(url: str, assets: dict, originals: dict) -> dict:
    environment = jinja2.Environment(
        trim_blocks=True,
        lstrip_blocks=True,
        undefined=jinja2.StrictUndefined,
        autoescape=False,
    )
    environment.globals["tan"] = lambda x: math.tan(math.radians(x))
    cases = []
    for name, asset in assets.items():
        scenarios = [("default", {})]
        if "DJ" in name:
            scenarios = [
                (f"{d}-Q{q}", {"U_FX": d, "U_Q": q})
                for d in ["闭口", "左开口", "右开口"]
                for q in [0.0, 12.5]
            ]
        elif "RKM" in name:
            scenarios += [("CT1017", {"U_RT": "CT1017"})]
        else:
            scenarios += [(tool, {"U_FT": tool}) for tool in ["5010", "5008"]]
        for label, overrides in scenarios:
            params = effective(asset, overrides)
            raw = environment.from_string(originals[name].decode("utf-8-sig")).render(
                params
            )
            old = "\n".join(line.strip() for line in raw.splitlines()) + "\n"
            expected = ZERO.sub(lambda m: m[0][1:], old)
            result = action(
                url, "nc.generate", {"template": identifier(name), "params": params}
            )
            text = result["data"]["text"]
            if text != expected:
                import difflib

                diff = "".join(
                    difflib.unified_diff(
                        expected.splitlines(True),
                        text.splitlines(True),
                        fromfile="V3",
                        tofile="2.2",
                    )
                )
                raise RuntimeError(f"{name}/{label} 对照不一致：\n{diff}")
            cases.append(
                {
                    "template": identifier(name),
                    "case": label,
                    "equal_after_declared_normalization": True,
                    "negative_zero_changes": len(ZERO.findall(old)),
                    "v3_sha256": sha(old.encode()),
                    "v2_sha256": sha(text.encode()),
                }
            )
        inspected = action(url, "template.inspect", {"template": identifier(name)})[
            "data"
        ]["variables"]
        if {v["name"] for v in inspected} != set(asset["schema"]["properties"]):
            raise RuntimeError("规格与变量提取不一致：" + name)
    rejected = []
    for name, overrides, label in [
        ("UZ_RKM_temp1.j2", {"U_ANG": 0}, "zero-entry-angle"),
        ("UZ_FKM_temp1.j2", {"U_ID": "INVALID"}, "invalid-register-id"),
        ("UZ_RKM_temp1.j2", {"U_RT": "CT1006"}, "missing-tool-lookup"),
        ("UZ_DJ_X.j2", {"U_Z": 1e-7}, "precision-underflow"),
        ("UZ_DJ_X.j2", {"R1": 1000.5}, "non-integer-spindle"),
    ]:
        try:
            action(
                url,
                "nc.generate",
                {
                    "template": identifier(name),
                    "params": {**assets[name]["defaults"], **overrides},
                    "lenient": True,
                },
            )
        except RuntimeError as error:
            rejected.append({"case": label, "error": json.loads(str(error))})
        else:
            raise RuntimeError("无效输入未被阻止：" + label)
    return {
        "comparison_cases": cases,
        "rejected_inputs": rejected,
        "statement": "仅验证语法/软件行为与已有文本；未进行机床、刀补、坐标变换或实际加工审核",
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/nctool"))
    parser.add_argument("--install-url")
    args = parser.parse_args()
    root = args.source_root.resolve()
    repo_raw = (root / "configs/variable_repo.json").read_bytes()
    cfg_raw = (root / "configs/templates.yaml").read_bytes()
    repo = json.loads(repo_raw)
    cfg = yaml.safe_load(cfg_raw)
    cfg = cfg.get("templates", cfg)
    commit = subprocess.check_output(
        ["git", "-C", str(root), "rev-parse", "HEAD"], text=True
    ).strip()
    originals, assets, ledger, records = {}, {}, [], []
    for name in FILES:
        raw = (root / "templates/INDEX G420" / name).read_bytes()
        originals[name] = raw
        asset, entry = convert(name, raw, repo, cfg, commit, root)
        assets[name] = asset
        ledger.append(entry)
        records.append({"group": "templates", "name": identifier(name), "asset": asset})
    collection = {"version": 1, "records": records}
    bundle = {
        "format": "nctool-workspace",
        "version": 1,
        "collections": {"template": collection},
        "manifest": [
            {
                "collection": "template",
                "provider": "template",
                "version": "2.0.0",
                "fingerprint": fingerprint(collection),
            }
        ],
    }
    with server(args.binary) as url:
        rpc(url, "bundle/validate", {"bundle": bundle})
        rpc(url, "bundle/import", {"bundle": bundle})
        report = verify(url, assets, originals)
    for name, asset in assets.items():
        write(args.output / "originals" / name, originals[name])
        write(args.output / "templates" / name, asset["source"].encode())
        write(
            args.output / "assets" / (Path(name).stem + ".template.json"),
            encoded(asset),
        )
    write(args.output / "source-config/variable_repo.json", repo_raw)
    write(args.output / "source-config/templates.yaml", cfg_raw)
    for source_license in root.glob("LICENSE*"):
        if source_license.is_file():
            write(
                args.output / "source-config" / source_license.name,
                source_license.read_bytes(),
            )
    write(args.output / "index-g420-uz-workspace.json", encoded(bundle))
    write(
        args.output / "conversion-manifest.json",
        encoded(
            {
                "source_commit": commit,
                "source_config_sha256": {
                    "variable_repo": sha(repo_raw),
                    "templates": sha(cfg_raw),
                },
                "templates": ledger,
            }
        ),
    )
    write(args.output / "verification.json", encoded(report))
    for name in FILES:
        if (root / "templates/INDEX G420" / name).read_bytes() != originals[name]:
            raise RuntimeError("源文件发生变化：" + name)
    if args.install_url:
        rpc(args.install_url, "bundle/validate", {"bundle": bundle})
        installed = rpc(args.install_url, "bundle/import", {"bundle": bundle})
        write(
            args.output / "installation.json",
            encoded({"url": args.install_url, "result": installed}),
        )
    print(
        json.dumps(
            {
                "templates": len(assets),
                "comparison_cases": len(report["comparison_cases"]),
                "rejected_inputs": len(report["rejected_inputs"]),
                "output": str(args.output.resolve()),
                "installed": bool(args.install_url),
            },
            ensure_ascii=False,
        )
    )


if __name__ == "__main__":
    main()
