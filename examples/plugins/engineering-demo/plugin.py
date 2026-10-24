"""Domain-neutral workbench demonstrating the same table / scene / project host."""
import json
import math
from pathlib import Path
from nctool_plugin import PluginServer, PluginError


def invoke(action, value, server, request_id):
    project = value["project"]
    items = []
    for index, (a, b) in enumerate(zip(project["points"], project["points"][1:])):
        if request_id in server.cancelled:
            raise PluginError({"code": "cancelled", "message": "cancelled", "diagnostics": []})
        items.append({"id": str(index), "entity": a["id"], "layer": "path", "kind": "line",
                      "start": [a["x"], a["y"]], "end": [b["x"], b["y"]],
                      "length": math.hypot(b["x"]-a["x"], b["y"]-a["y"])})
    scene = {"format_version": 1, "axes": {"x": "x", "y": "y", "y_scale": 1},
             "items": items, "path": [item["id"] for item in items]}
    result = {"data": {"result": {"valid": True}, "scene": scene, "replay_input": value}}
    if action == "drawing.generate":
        result["artifacts"] = [{"name": "drawing.json", "media_type": "application/json",
                                "text": json.dumps(project, ensure_ascii=False, indent=2)}]
    return result


if __name__ == "__main__":
    PluginServer(json.loads(Path(__file__).with_name("plugin.json").read_text(encoding="utf-8")), invoke).run()
