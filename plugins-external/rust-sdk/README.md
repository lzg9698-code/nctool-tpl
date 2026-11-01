# nctool-external-sdk — Rust 外部插件 SDK（协议 v2）

NCtool 外部插件的最小 Rust 客户端实现，从首个真实插件 nc-undercut 中抽出。覆盖协议 v2 全部传输面：newline 分隔的 stdin/stdout JSON-RPC、`initialize`/`describe`/`invoke` 请求、`cancel`/`shutdown` 通知、反向 `host.call` 回调与进程退出语义。

## 使用

```toml
[dependencies]
nctool-external-sdk = { path = "../rust-sdk" }
serde_json = "1"
```

```rust
use nctool_external_sdk::Server;

fn main() {
    let manifest = Server::manifest_from(&std::path::Path::new("plugin.json")).unwrap();
    let server = Server::new(manifest, |action, input, server, request_id| {
        // 实现动作；server.call(request_id, "template.render", input) 反向调宿主服务，
        // server.is_cancelled(request_id) / check_cancelled 轮询取消。
        todo!()
    });
    server.run(); // stdin EOF / shutdown 时自行退出进程
}
```

完整示例见 [`nc-undercut`](../nc-undercut)（动作、资产集合、`host.call`、取消/超时恢复）。

## 实现要点

- **invoke 在 worker 线程**：处理线程阻塞等待 `host.call` 响应时，stdin 读循环仍须继续路由宿主回帧；同线程等待会死锁。
- **EOF / shutdown 即退出**：`run` 在 stdin 关闭或收到 `shutdown` 通知后释放所有 `host.call` 等待者并 `exit(0)`，这是宿主进程回收所依赖的语义。
- **`cancel` 是通知**：SDK 维护已取消请求 id 集合，插件在 invoke 中轮询 `is_cancelled`/`check_cancelled` 后自行返回 `cancelled` 错误。
- **字符串 id**：宿主请求 id 是字符串（`host-N`），响应原样回显；插件侧 `host.call` id 格式不限。
- **describe 回显清单**：插件直接把 `plugin.json` 的 `descriptor`/`actions` 原样回显，保证与安装清单逐字节语义一致。

该 crate 独立构建（自带 `[workspace]`），不依赖宿主 workspace，path 依赖即可用于新的 Rust 外部插件。
