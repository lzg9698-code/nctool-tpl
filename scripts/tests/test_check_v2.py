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
    metadata = {
        name: {"name": name, "dependencies": [
            {"name": dependency, "kind": None} for dependency in sorted(dependencies)
        ]}
        for name, dependencies in CHECK.NEUTRAL_DEPENDENCIES.items()
    }
    metadata["nctool-tpl"] = {
        "name": "nctool-tpl", "dependencies": [{"name": "minijinja", "kind": None}],
    }
    metadata["nctool-cli"] = {
        "name": "nctool-cli",
        "features": {"default": [], "math": ["dep:" + CHECK.MATH_PACKAGE]},
        "dependencies": [
            {"name": CHECK.MATH_PACKAGE, "kind": None, "optional": True},
        ] + [
            {"name": "nctool-plugin-" + name, "kind": None, "optional": False}
            for name in ("template", "parameters")
        ],
    }
    return metadata


TEMPLATE_TREE = "nctool-plugin-template nctool-plugin-parameters"
MATH_TREE = TEMPLATE_TREE + " " + CHECK.MATH_PACKAGE
WORKSPACE_TREE = MATH_TREE


class ArchitectureTest(unittest.TestCase):
    def check(self, metadata=None, workspace=WORKSPACE_TREE, template=TEMPLATE_TREE, math=MATH_TREE):
        CHECK.check_architecture(metadata or packages(), workspace, template, math)

    def test_optional_math_and_pure_engine(self):
        self.check()

    def test_retired_nc_packages_are_rejected_everywhere(self):
        for name in CHECK.RETIRED_PACKAGES:
            with self.subTest(package=name):
                metadata = packages()
                metadata[name] = {"name": name, "dependencies": []}
                with self.assertRaises(AssertionError):
                    self.check(metadata)
                with self.assertRaises(AssertionError):
                    self.check(workspace=WORKSPACE_TREE + " " + name)
                with self.assertRaises(AssertionError):
                    self.check(metadata, workspace=WORKSPACE_TREE)

    def test_domain_implementations_stay_out_of_neutral_dependencies(self):
        for package in CHECK.NEUTRAL_DEPENDENCIES:
            for dependency in CHECK.RETIRED_PACKAGES | {"nctool-plugin-nc-model"}:
                with self.subTest(package=package, dependency=dependency):
                    metadata = packages()
                    metadata[package]["dependencies"].append({"name": dependency, "kind": None})
                    with self.assertRaises(AssertionError):
                        self.check(metadata)
        metadata = packages()
        metadata["nctool-runtime"]["dependencies"].append({"name": "nctool-plugin-nc", "kind": "dev"})
        self.check(metadata)

    def test_nc_bundle_feature_is_gone(self):
        metadata = packages()
        metadata["nctool-cli"]["features"]["nc-bundle"] = ["dep:nctool-plugin-nc"]
        with self.assertRaises(AssertionError):
            self.check(metadata)

    def test_math_is_the_only_optional_feature_and_complete(self):
        for features in [{"default": ["math"]}, {"math": []}]:
            with self.subTest(features=features):
                metadata = packages()
                metadata["nctool-cli"]["features"].update(features)
                with self.assertRaises(AssertionError):
                    self.check(metadata)
        with self.assertRaises(AssertionError):
            self.check(template=TEMPLATE_TREE + " " + CHECK.MATH_PACKAGE)
        with self.assertRaises(AssertionError):
            self.check(math=TEMPLATE_TREE)
        metadata = packages()
        next(d for d in metadata["nctool-cli"]["dependencies"] if d["name"] == CHECK.MATH_PACKAGE)["optional"] = False
        with self.assertRaises(AssertionError):
            self.check(metadata)

    def test_default_build_keeps_template_and_parameters(self):
        for name in ("nctool-plugin-template", "nctool-plugin-parameters"):
            with self.subTest(name=name):
                with self.assertRaises(AssertionError):
                    self.check(template=TEMPLATE_TREE.replace(name, ""))
                metadata = packages()
                next(d for d in metadata["nctool-cli"]["dependencies"] if d["name"] == name)["optional"] = True
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

    def test_shared_parameter_module_keeps_generic_helpers_only(self):
        """共享参数模块必须保留通用 helper；NC 专用计算不应再存在。"""
        module = CHECK.ROOT / "ui/src/lib/managedParameters.ts"
        self.assertTrue(module.exists(), "共享参数模块缺失，引用方会失效")
        text = module.read_text(encoding="utf-8")
        for expected in ["pointerTokens", "pointerValue", "applyOverrides", "managedRequest", "managedFields"]:
            with self.subTest(exported=expected):
                self.assertIn(f"export function {expected}", text)

    def test_retired_nc_sources_must_not_come_back(self):
        for name in CHECK.RETIRED_SOURCES:
            with self.subTest(source=name):
                self.assertFalse(
                    (CHECK.ROOT / name).exists(),
                    f"退役加工实现重新出现：{name}",
                )


if __name__ == "__main__":
    unittest.main()
