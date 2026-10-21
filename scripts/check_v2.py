#!/usr/bin/env python3
"""Check domain-neutral architecture, independent math and generated WebUI without editing sources."""
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
# The only optional builtin left is the domain-neutral math extension. Every NC
# package must be gone from the workspace, dependencies and both build trees.
MATH_PACKAGE = "nctool-plugin-math"
RETIRED_PACKAGES = {
    f"nctool-plugin-{name}" for name in ("nc", "process", "sync-turning", "undercut")
}
# Internal production dependencies are explicit: domain implementations stay out of
# infrastructure and the generic template/parameter plugins.
NEUTRAL_DEPENDENCIES = {
    "nctool-assets": set(),
    "nctool-plugin-sdk": {"nctool-assets", "nctool-tpl"},
    "nctool-runtime": {"nctool-assets", "nctool-plugin-sdk"},
    "nctool-plugin-template": {"nctool-assets", "nctool-plugin-sdk", "nctool-tpl"},
    "nctool-plugin-parameters": {"nctool-assets", "nctool-plugin-sdk"},
}
RETIRED_SOURCES = (
    "plugins/nc",
    "plugins/process",
    "plugins/sync-turning",
    "plugins/undercut",
    "ui/src/plugins/NcViews.tsx",
    "ui/src/plugins/SyncTurningView.tsx",
    "ui/src/plugins/TurningWorkbench.tsx",
    "ui/src/plugins/UndercutView.tsx",
    "ui/src/lib/turningModel.ts",
    "ui/src/lib/syncAp2.ts",
    "ui/src/components/SpecializedTemplateEditor.tsx",
    "ui/src/components/TrajectoryPresetEditor.tsx",
)


def run(*command):
    result = subprocess.run(
        command, cwd=ROOT, check=True, capture_output=True,
        text=True, encoding="utf-8",
    )
    return result.stdout


def check_architecture(packages, workspace_tree, template_tree, math_tree):
    """Check production boundaries and the two supported CLI build compositions."""
    assert not {"nctool-gui", "nctool-core"} & packages.keys()
    assert not RETIRED_PACKAGES & packages.keys(), RETIRED_PACKAGES & packages.keys()
    retired_names = "|".join(sorted(re.escape(name) for name in RETIRED_PACKAGES))
    retired_tree = re.compile(rf"\b(?:{retired_names})\b")
    for tree in (workspace_tree, template_tree, math_tree):
        assert not re.search(r"\b(?:tauri(?:-[\w-]+)?|nctool-gui)\b", tree), tree
        assert not retired_tree.search(tree), tree
    for name, allowed in NEUTRAL_DEPENDENCIES.items():
        dependencies = {
            d["name"] for d in packages[name]["dependencies"]
            if d["kind"] != "dev" and d["name"].startswith("nctool-")
        }
        assert dependencies == allowed, (name, dependencies, allowed)
    assert {
        d["name"] for d in packages["nctool-tpl"]["dependencies"] if d["kind"] != "dev"
    } == {"minijinja"}
    cli = packages["nctool-cli"]
    assert cli["features"].get("default") == [], cli["features"]
    # The only optional feature is math; there is no nc-bundle and no nc profile.
    assert "nc-bundle" not in cli["features"], cli["features"]
    assert set(cli["features"].get("math", [])) == {"dep:" + MATH_PACKAGE}, cli["features"]
    math = next(d for d in cli["dependencies"] if d["name"] == MATH_PACKAGE)
    assert math["optional"], math
    assert MATH_PACKAGE not in template_tree, template_tree
    assert MATH_PACKAGE in math_tree, math_tree
    assert MATH_PACKAGE in workspace_tree, workspace_tree
    for name in ("nctool-plugin-template", "nctool-plugin-parameters"):
        dependency = next(d for d in cli["dependencies"] if d["name"] == name)
        assert not dependency["optional"], dependency
        assert name in template_tree and name in math_tree, name


def main():
    metadata = json.loads(run("cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"))
    packages = {package["name"]: package for package in metadata["packages"]}
    check_architecture(
        packages,
        run("cargo", "tree", "--workspace", "--all-features", "--edges", "normal", "--locked"),
        run("cargo", "tree", "-p", "nctool-cli", "--no-default-features", "--edges", "normal", "--locked"),
        run("cargo", "tree", "-p", "nctool-cli", "--no-default-features", "--features", "math", "--edges", "normal", "--locked"),
    )
    for name in ("gui", "启动GUI.bat"):
        assert not (ROOT / name).exists(), f"Retired source or entry point must not be present: {name}"
    for name in RETIRED_SOURCES:
        assert not (ROOT / name).exists(), f"Retired NC source must not be present: {name}"
    renderer = (ROOT / "src/renderer.rs").read_text(encoding="utf-8")
    assert "with_v3_whitespace" not in renderer
    assert not re.search(r'add_filter\("(?:nc_|sin|cos|tan|sqrt)', renderer)
    assert not (ROOT / "src/lint.rs").exists()
    assert (ROOT / "sdk/python/nctool_plugin.py").read_bytes() == (ROOT / "examples/plugins/python-report/nctool_plugin.py").read_bytes()
    run("node", "scripts/build_ui.mjs", "--check")
    retired_chunks = re.compile(r"^(?:NcViews|TurningWorkbench|SyncTurningView|UndercutView|SpecializedTemplateEditor|TrajectoryPresetEditor)-")
    for directory in ("ui/dist/assets", "cli/ui/assets"):
        for asset in (ROOT / directory).iterdir():
            assert not retired_chunks.match(asset.name), f"Retired NC chunk: {asset}"
            if asset.suffix == ".js":
                run("node", "--check", str(asset))
    for script in ("build_ui.mjs", "browser_v2.cjs", "browser_review_fixes.cjs", "browser_generic_parameters.cjs"):
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
    print("PASS: domain-neutral dependencies, optional math only, no NC source/dependency/feature, scoped engine, Tauri-free workspace, Python example, generated WebUI/JavaScript and package contents")


if __name__ == "__main__":
    main()
