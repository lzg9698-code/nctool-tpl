"""Test-only fixture for plugin-author protocol acceptance; not a product action."""
import json
import os
from pathlib import Path
import time
from nctool_plugin import PluginError, PluginServer


def invoke(action, value, server, request_id):
    if action == "probe.echo":
        rendered = server.call(request_id, "template.render", {"source":"{{ value }}", "context":{"value":value["text"]}})
        return {"data":{"echo":rendered["data"]["text"],"process_id":os.getpid()}}
    if action == "probe.wait":
        deadline = time.monotonic() + value["milliseconds"] / 1000
        while time.monotonic() < deadline:
            if not value.get("ignore_cancel") and request_id in server.cancelled:
                raise PluginError({"code":"cancelled","message":"fixture cancelled","diagnostics":[]})
            time.sleep(0.01)
        return {"data":{"process_id":os.getpid()}}
    raise ValueError("unknown fixture action")


if __name__ == "__main__":
    PluginServer(json.loads(Path(__file__).with_name("plugin.json").read_text(encoding="utf-8")), invoke).run()
