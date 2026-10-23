"""Validate author plans and safe host-boundary behavior without launching user code."""
import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("check_plugin",ROOT / "scripts/check_plugin.py")
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class PluginCheck(unittest.TestCase):
    def plan(self):
        return json.loads((ROOT / "examples/plugins/protocol-probe/acceptance.json").read_text(encoding="utf-8"))

    def test_plan_requires_all_lifecycle_cases_and_callback_expectations(self):
        CHECK.validate_plan(self.plan())
        for missing in ("success","cancel","invalid","timeout","unresponsive","identity_field"):
            plan=self.plan()
            del plan[missing]
            with self.subTest(missing=missing), self.assertRaises(CHECK.CheckFailure):
                CHECK.validate_plan(plan)
        plan=self.plan()
        plan["success"]["callback_actions"]="not-an-array"
        with self.assertRaises(CHECK.CheckFailure):
            CHECK.validate_plan(plan)

    def test_bad_deadlines_fail_before_starting_any_package(self):
        plan=self.plan()
        for value in (-1,0,30_001):
            plan["timeout_ms"]=value
            with patch.object(CHECK.subprocess,"Popen") as spawn:
                with self.assertRaises(CHECK.CheckFailure):
                    CHECK.verify(Path("unused"),Path("unused"),plan)
                spawn.assert_not_called()

    def test_subset_checks_nested_values_without_coercing_large_integers(self):
        self.assertTrue(CHECK.subset({"a":{"n":9007199254740993},"other":1},{"a":{"n":9007199254740993}}))
        self.assertFalse(CHECK.subset({"a":{"n":9007199254740992}},{"a":{"n":9007199254740993}}))
