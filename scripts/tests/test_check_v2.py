"""Architecture guard regressions without invoking Cargo or rebuilding WebUI."""
import copy
import importlib.util
from pathlib import Path
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "check_v2.py"
SPEC = importlib.util.spec_from_file_location("check_v2", SCRIPT)
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


def packages():
    return {
        "nctool-tpl": {
            "name": "nctool-tpl",
            "dependencies": [{"name": "minijinja", "kind": None}],
        },
        "nctool-cli": {
            "name": "nctool-cli",
            "features": {"default": [], "math": ["dep:nctool-plugin-math"]},
            "dependencies": [{"name": "nctool-plugin-math", "kind": None, "optional": True}],
        },
    }


class ArchitectureTest(unittest.TestCase):
    def check(self, metadata=None, workspace="generic", template="template", math="nctool-plugin-math"):
        CHECK.check_architecture(metadata or packages(), workspace, template, math)

    def test_optional_math_and_pure_engine(self):
        self.check()

    def test_retired_package_or_dependency_is_rejected(self):
        for name in CHECK.RETIRED_PACKAGES:
            for field in ["package", "dependency", "tree"]:
                with self.subTest(name=name, field=field):
                    metadata = packages()
                    if field == "package":
                        metadata[name] = {"name": name, "dependencies": []}
                    elif field == "dependency":
                        metadata["nctool-cli"]["dependencies"].append({"name": name})
                    with self.assertRaises(AssertionError):
                        self.check(metadata, workspace=name if field == "tree" else "generic")

    def test_compiled_math_does_not_become_default(self):
        for features in [{"default": ["math"]}, {"math": []}, {"nc-bundle": []}]:
            with self.subTest(features=features):
                metadata = packages()
                metadata["nctool-cli"]["features"].update(features)
                with self.assertRaises(AssertionError):
                    self.check(metadata)
        with self.assertRaises(AssertionError):
            self.check(template="nctool-plugin-math")
        with self.assertRaises(AssertionError):
            self.check(math="template")
        metadata = packages()
        metadata["nctool-cli"]["dependencies"][0]["optional"] = False
        with self.assertRaises(AssertionError):
            self.check(metadata)

    def test_engine_production_dependencies_stay_minimal(self):
        metadata = packages()
        metadata["nctool-tpl"]["dependencies"].append({"name": "criterion", "kind": "dev"})
        self.check(metadata)
        changed = copy.deepcopy(metadata)
        changed["nctool-tpl"]["dependencies"].append({"name": "serde_json", "kind": None})
        with self.assertRaises(AssertionError):
            self.check(changed)

    def test_legacy_desktop_dependency_is_rejected(self):
        for tree in ["tauri v2", "tauri-runtime v2", "nctool-gui v1"]:
            with self.subTest(tree=tree), self.assertRaises(AssertionError):
                self.check(workspace=tree)

    def test_shared_parameter_module_stays_free_of_retired_tooling_semantics(self):
        """managedParameters.ts 是重用的通用模块，不能再变成退役的轴类刀具槽选择。

        该文件名曾因只服务退役的 slots/tool_id/inputs_draft 语义被列入 RETIRED_UI。
        WebUI 重构重用它承载通用共享参数（JSON Pointer 与请求构造），因此移出清单；
        但退役语义不得悄悄回来，否则应重新评估该模块是否还该保留这个文件名。
        """
        module = CHECK.ROOT / "ui/src/lib/managedParameters.ts"
        self.assertTrue(module.exists(), "共享参数模块缺失，引用方会失效")
        text = module.read_text(encoding="utf-8")
        for token in ["slots", "tool_id", "inputs_draft", "overrides_draft", "validatedSelection"]:
            with self.subTest(token=token):
                self.assertNotIn(token, text)
        for expected in ["pointerTokens", "pointerValue", "applyOverrides"]:
            with self.subTest(exported=expected):
                self.assertIn(f"export function {expected}", text)


if __name__ == "__main__":
    unittest.main()
