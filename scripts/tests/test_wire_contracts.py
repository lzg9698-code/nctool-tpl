"""The same wire fixture is consumed by Rust and TypeScript; exercise SDK forwarding."""
import importlib.util
import io
import json
from pathlib import Path
import threading
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("nctool_sdk", ROOT / "sdk/python/nctool_plugin.py")
sdk = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sdk)


class WireContracts(unittest.TestCase):
    def test_shared_fixture_and_error_forwarding(self):
        f = json.loads((ROOT / "tests/fixtures/contracts/v2.json").read_text(encoding="utf-8"))
        self.assertEqual(f["protocol_version"], sdk.PROTOCOL_VERSION)
        self.assertEqual(f["asset"]["asset"]["defaults"]["n"], 2**64 - 1)
        self.assertEqual(f["result"]["data"]["n"], 9007199254740993)
        # Avoid replacing process stdin/stdout: invoke the real response method with
        # an in-memory transport, exactly as an exception from host.call is forwarded.
        server = sdk.PluginServer.__new__(sdk.PluginServer)
        server.manifest = {"descriptor": f["plugin"], "actions": [f["action"]]}
        messages = []
        server.send = messages.append
        def invoke(*_):
            raise sdk.PluginError(f["error"])
        server.invoke = invoke
        server.respond({"id":"host-1", "method":"invoke", "params":{"action":"report.render", "input":{}}})
        self.assertEqual(messages[0]["error"]["data"], f["error"])
        self.assertEqual(json.loads(json.dumps(f)), f)

    def test_reverse_call_keeps_structured_error(self):
        import concurrent.futures
        from unittest.mock import patch
        detail = {"code":"template_parse","message":"syntax","diagnostics":[{"level":"error","code":"template_parse","message":"syntax","template":"child.j2","line":2,"column":3}]}
        server = sdk.PluginServer.__new__(sdk.PluginServer)
        future = concurrent.futures.Future()
        server.pending_lock = threading.Lock()
        server.pending = {"plugin-1": future}
        class Pool:
            def shutdown(self, **_): pass
        server.pool = Pool()
        response = json.dumps({"jsonrpc":"2.0", "id":"plugin-1", "error":{"code":-32000,"message":"syntax","data":detail}})
        with patch("sys.stdin", io.StringIO(response + "\n")):
            server.run()
        with self.assertRaises(sdk.PluginError) as raised:
            future.result()
        self.assertEqual(raised.exception.detail, detail)
