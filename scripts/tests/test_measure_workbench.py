"""Independent checks for the reproducible performance report helper."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    "measure_workbench", Path(__file__).resolve().parents[1] / "measure_workbench.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class CompressionTests(unittest.TestCase):
    def test_counts_served_assets_and_checks_round_trip(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "assets").mkdir()
            (root / "index.html").write_bytes(b"<html>hello</html>")
            (root / "assets/app.js").write_bytes(b"const repeated = 1;\n" * 100)
            (root / "assets.rs").write_bytes(b"not served")
            report = MODULE.compression(root)
            self.assertEqual(report["files"], 2)
            self.assertEqual(report["raw_bytes"], 2018)
            self.assertLess(report["gzip_bytes"], report["raw_bytes"])
            self.assertGreaterEqual(report["decompress_all_ms_median"], 0)


class InvokeTimeoutTests(unittest.TestCase):
    def test_timed_out_measurement_kills_the_child_and_still_fails(self):
        process = mock.Mock()
        process.communicate.side_effect = [
            subprocess.TimeoutExpired(cmd="nctool", timeout=60),
            (b"", b""),
        ]
        with mock.patch.object(MODULE.subprocess, "Popen", return_value=process):
            with self.assertRaises(subprocess.TimeoutExpired):
                MODULE.invoke(
                    Path("nctool"), Path("home"), Path("workspace"), "x", {}, []
                )
        # A hung measurement must not leak the CLI process.
        process.kill.assert_called_once_with()
        self.assertEqual(process.communicate.call_count, 2)


if __name__ == "__main__":
    unittest.main()
