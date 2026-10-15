#!/usr/bin/env python3
"""Check generic architecture, optional math and generated WebUI without editing sources."""
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
RETIRED_PLUGINS = {"nc", "process", "sync-turning", "undercut"}
RETIRED_PACKAGES = {f"nctool-plugin-{name}" for name in RETIRED_PLUGINS}
RETIRED_UI = (
    "ui/src/plugins/NcViews.tsx",
    "ui/src/plugins/TurningWorkbench.tsx",
    "ui/src/plugins/SyncTurningView.tsx",
    "ui/src/plugins/UndercutView.tsx",
    "ui/src/components/SpecializedTemplateEditor.tsx",
    "ui/src/components/TrajectoryPresetEditor.tsx",
    "ui/src/lib/turningModel.ts",
    "ui/src/lib/syncAp2.ts",
)
# ui/src/lib/managedParameters.ts 曾在此清单中要求删除。该文件名在加工能力退役时
# 只用于已退出的轴类输入选择（slots/tool_id/inputs_draft），已随退役内容一并移除；
# 但 WebUI 重构重新把它用作通用共享参数的 JSON Pointer 工具与请求构造模块
# （originLabel/pointerTokens/pointerValue/applyOverrides/removePointer/
# managedRequest/managedFields），由 ui/src/features/workbench/ParametersPane.tsx
# 及其测试实际引用。同名的两类内容不同：退役的是轴类刀具槽选择，不是这个模块本身。
# 因此本项已从“必须移除”改为“内容不再包含退役语义”，由 managedParameters 自身测试
# 约束其导出面；若将来确认该模块也应退役，需同步迁移 ParametersPane 的引用后再加回。
RETIRED_CHUNK = re.compile(
    r"^(?:NcViews|TurningWorkbench|SyncTurningView|UndercutView|"
    r"SpecializedTemplateEditor|TrajectoryPresetEditor)-"
)


def run(*command):
    result = subprocess.run(
        command, cwd=ROOT, check=True, capture_output=True,
        text=True, encoding="utf-8",
    )
    return result.stdout


def check_architecture(packages, workspace_tree, template_tree, math_tree):
    """Keep dependency checks separately testable without compiling or regenerating UI."""
    assert not ({"nctool-gui", "nctool-core"} | RETIRED_PACKAGES) & packages.keys()
    for package in packages.values():
        dependencies = {dependency["name"] for dependency in package["dependencies"]}
        assert not dependencies & RETIRED_PACKAGES, (package["name"], dependencies)
    for tree in (workspace_tree, template_tree, math_tree):
        assert not re.search(r"\b(?:tauri(?:-[\w-]+)?|nctool-gui)\b", tree), tree
        for name in RETIRED_PACKAGES:
            assert not re.search(r"\b" + re.escape(name) + r"\b", tree), tree
    assert "nctool-plugin-math" not in template_tree, template_tree
    assert "nctool-plugin-math" in math_tree, math_tree
    cli = packages["nctool-cli"]
    assert cli["features"].get("default") == [], cli["features"]
    assert cli["features"].get("math") == ["dep:nctool-plugin-math"], cli["features"]
    assert "nc-bundle" not in cli["features"], cli["features"]
    math = next(d for d in cli["dependencies"] if d["name"] == "nctool-plugin-math")
    assert math["optional"], math
    assert {
        d["name"] for d in packages["nctool-tpl"]["dependencies"] if d["kind"] is None
    } == {"minijinja"}


def main():
    metadata = json.loads(run("cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"))
    packages = {package["name"]: package for package in metadata["packages"]}
    check_architecture(
        packages,
        run("cargo", "tree", "--workspace", "--all-features", "--edges", "normal", "--locked"),
        run("cargo", "tree", "-p", "nctool-cli", "--no-default-features", "--edges", "normal", "--locked"),
        run("cargo", "tree", "-p", "nctool-cli", "--no-default-features", "--features", "math", "--edges", "normal", "--locked"),
    )
    for name in ("gui", "启动GUI.bat", *[f"plugins/{name}" for name in RETIRED_PLUGINS], *RETIRED_UI):
        assert not (ROOT / name).exists(), f"Retired source or entry point must not be present: {name}"
    renderer = (ROOT / "src/renderer.rs").read_text(encoding="utf-8")
    assert "with_v3_whitespace" not in renderer
    assert not re.search(r'add_filter\("(?:nc_|sin|cos|tan|sqrt)', renderer)
    assert not (ROOT / "src/lint.rs").exists()
    assert (ROOT / "sdk/python/nctool_plugin.py").read_bytes() == (ROOT / "examples/plugins/python-report/nctool_plugin.py").read_bytes()
    run("node", "scripts/build_ui.mjs", "--check")
    for directory in ("ui/dist/assets", "cli/ui/assets"):
        for asset in (ROOT / directory).iterdir():
            assert not RETIRED_CHUNK.match(asset.name), f"Retired WebUI chunk: {asset}"
            if asset.suffix == ".js":
                run("node", "--check", str(asset))
    for script in ("build_ui.mjs", "browser_v2.cjs", "browser_review_fixes.cjs", "browser_ui_refactor.cjs"):
        run("node", "--check", "scripts/" + script)
    html = (ROOT / "cli/ui/index.html").read_text(encoding="utf-8")
    assert "/assets/" in html and 'type="module"' in html
    assert (ROOT / "ui/src/App.tsx").exists()
    source = (ROOT / "ui/src/lib/api.ts").read_text(encoding="utf-8")
    assert "/api/v2/" in source and "encode(body)" in source
    assert (ROOT / "cli/ui/assets.rs").exists()
    package = run("cargo", "package", "-p", "nctool-tpl", "--list", "--allow-dirty", "--offline", "--locked")
    assert not any(name.startswith(("cli/", "core/", "plugins/", "gui/", "templates/", "docs/", "ui/", "runtime/", "assets/", "sdk/", "shaft_programmer/")) for name in package.splitlines())
    manifest = json.loads((ROOT / "examples/plugins/python-report/plugin.json").read_text(encoding="utf-8"))
    assert manifest["descriptor"]["requires"] == [{"id": "template.render", "version": 1}]
    print("PASS: pure template dependency tree, explicitly compiled optional math, scoped engine, retired NC source/dependency/chunk removal, Tauri-free workspace, Python example, generated WebUI/JavaScript and package contents")


if __name__ == "__main__":
    main()
