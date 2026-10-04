"""A non-manufacturing plugin: compute a total and ask the generic template service for a report."""
import json
import math
from pathlib import Path
from nctool_plugin import PluginServer


def invoke(action, input_data, server, request_id):
    if action != "report.compute":
        raise ValueError("unknown action")
    total = sum(input_data["values"])
    if not math.isfinite(total):
        raise ValueError("total must be finite")
    rendered = server.call(request_id, "template.render", {
        "source": "{{ title }}\nTotal: {{ total }}\n{% for value in values %}- {{ value }}\n{% endfor %}",
        "context": {"title": input_data.get("title", "Report"), "total": total,
                    "values": input_data["values"]}})
    return {"data": {"total": total, "text": rendered["data"]["text"]},
            "artifacts": rendered["artifacts"], "diagnostics": rendered["diagnostics"]}


if __name__ == "__main__":
    manifest = json.loads(Path(__file__).with_name("plugin.json").read_text(encoding="utf-8"))
    PluginServer(manifest, invoke).run()
