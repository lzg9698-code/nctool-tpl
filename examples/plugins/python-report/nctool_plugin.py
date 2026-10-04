"""Small stdlib-only client/server for NCtool's bidirectional JSON-RPC protocol."""
import concurrent.futures
import json
import sys
import threading


class PluginServer:
    def __init__(self, manifest, invoke):
        sys.stdin.reconfigure(encoding="utf-8")
        sys.stdout.reconfigure(encoding="utf-8")
        self.manifest = manifest
        self.invoke = invoke
        self.config = {}
        self.write_lock = threading.Lock()
        self.pending_lock = threading.Lock()
        self.pending = {}
        self.counter = 0
        self.cancelled = set()
        self.pool = concurrent.futures.ThreadPoolExecutor(max_workers=8)

    def send(self, message):
        encoded = json.dumps(message, ensure_ascii=False, allow_nan=False)
        with self.write_lock:
            print(encoded, flush=True)

    def call(self, parent_id, service, input_data, version=1):
        with self.pending_lock:
            self.counter += 1
            request_id = "plugin-" + str(self.counter)
            future = concurrent.futures.Future()
            self.pending[request_id] = future
        self.send({"jsonrpc": "2.0", "id": request_id, "method": "host.call",
                   "params": {"parent_id": parent_id, "service": service,
                              "version": version, "input": input_data}})
        try:
            return future.result(timeout=30)
        finally:
            with self.pending_lock:
                self.pending.pop(request_id, None)

    def respond(self, message):
        request_id = message["id"]
        method = message["method"]
        try:
            if method == "initialize":
                params = message["params"]
                if params["protocol_version"] != 1:
                    raise ValueError("unsupported protocol version")
                self.config = params.get("config", {})
                result = {"plugin_id": self.manifest["descriptor"]["id"], "protocol_version": 1}
            elif method == "describe":
                result = {"descriptor": self.manifest["descriptor"], "actions": self.manifest["actions"]}
            elif method == "invoke":
                params = message["params"]
                result = self.invoke(params["action"], params["input"], self, request_id)
            else:
                raise ValueError("unsupported method: " + method)
            self.send({"jsonrpc": "2.0", "id": request_id, "result": result})
        except Exception as error:
            self.send({"jsonrpc": "2.0", "id": request_id,
                       "error": {"code": -32000, "message": str(error),
                                 "data": {"code": "plugin_error", "message": str(error), "diagnostics": []}}})

    def run(self):
        try:
            for line in sys.stdin:
                message = json.loads(line)
                method = message.get("method")
                if method == "shutdown":
                    break
                if method == "cancel":
                    self.cancelled.add(message["params"]["id"])
                elif method is not None:
                    self.pool.submit(self.respond, message)
                else:
                    with self.pending_lock:
                        future = self.pending.get(message.get("id"))
                    if future is not None and not future.done():
                        if "error" in message:
                            future.set_exception(RuntimeError(message["error"]["message"]))
                        else:
                            future.set_result(message["result"])
        finally:
            with self.pending_lock:
                for future in self.pending.values():
                    if not future.done():
                        future.set_exception(RuntimeError("host disconnected"))
            self.pool.shutdown(wait=False, cancel_futures=True)
