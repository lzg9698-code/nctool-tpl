# nc-undercut — 外部插件（协议 v2 真实领域验证）

把已退役的内置「越程槽（undercut）」能力以**独立 Rust 外部插件**形式回归：几何计算、轨迹计划与标准表全部在插件内完成，最终程序文本通过 `host.call → template.render` 生成。它是第一个覆盖协议 v2 各面的真实领域插件，也是 Rust 外部插件客户端（`src/rpc.rs`）的参考实现。

## 目录结构

```
plugins-external/nc-undercut/
├── plugin.json          # 描述文件 + 动作清单 + limits（宿主以此为校验基准）
├── src/
│   ├── rpc.rs           # 协议 v2 客户端：stdin/stdout JSON-RPC、host.call、取消、shutdown
│   ├── calculation.rs   # ES/FS 几何计算（移植自退役内置实现，行为不变）
│   ├── trajectory.rs    # Plan/Block/Word 轨迹与 V3 兼容策略
│   ├── plugin.rs        # 动作实现与资产集合（自定义标准）
│   └── main.rs          # 入口：读 plugin.json，启动 Server
├── standards/v3-grooves.json   # 内置标准表（退役资产 verbatim 恢复）
├── templates/program.j2        # 程序模板（经 template.render 渲染）
├── acceptance.json             # check_plugin.py 验收计划
├── build.sh                    # 构建并把可安装包组装进 dist/
└── data/                       # 运行时写入：导入的自定义标准（包内为空目录）
```

## 构建与安装

```bash
cd plugins-external/nc-undercut
bash build.sh                                     # 产出 dist/（可直接安装的插件包）

nctool --home <HOME> --workspace <WS> plugins install plugins-external/nc-undercut/dist
nctool --home <HOME> --workspace <WS> plugins enable nc-undercut
cat input.json | nctool --home <HOME> --workspace <WS> run nc-undercut.generate \
    --input - --out-dir out/
```

## 动作

| 动作 | 说明 |
| --- | --- |
| `nc-undercut.describe` | 能力说明：批量输入 schema、单位、标准键、轨迹参数 schema |
| `nc-undercut.standards` | 合并后标准表（内置 + 自定义）与指纹 |
| `nc-undercut.prepare` | 校验并计算全部槽行：上下文、轨迹计划、预览、通道程序文本 |
| `nc-undercut.generate` | `prepare` 之上经 `template.render` 生成 `.SPF` 工件（含 `replay_input`） |
| `nc-undercut.standards.export` / `.import` | `nc-undercut.standards` 资产集合的导出/校验导入/回滚 |
| `nc-undercut.wait` | 协议验收夹具（取消/超时/占满并发），不进面板 |

`generate` 返回 `data.replay_input`（原始输入回显），工作台历史可用它重执行。

## 验收

```bash
python3 scripts/check_plugin.py \
  --binary target/release/nctool \
  --source plugins-external/nc-undercut/dist \
  --plan plugins-external/nc-undercut/acceptance.json \
  --output /tmp/nc-undercut-acceptance.json
```

覆盖：握手与 describe 一致性、`template.render` 反向回调可观测、输入 schema 拒绝、请求取消、超时 + 迟到响应同进程复用、并发槽占满后进程重启恢复。

## 实现要点与协议摩擦记录

`src/rpc.rs` 是可以原样抽出的最小 Rust 客户端，实现中确认/记录的点：

- **stdin 读循环与请求处理必须分离**：invoke 处理在 worker 线程，否则处理线程阻塞在 `host.call` 等待时，宿主回来的响应帧无人读取，死锁。
- **stdin EOF / shutdown 即退出**：收到 EOF 或 `shutdown` 通知必须尽快 `exit`，否则宿主按超时后杀进程路径处理；在途 worker 会被打断（可接受，宿主已在拆线）。
- **`cancel` 是通知**：插件需要自己维护「已取消请求 id」集合并主动轮询（`Server::is_cancelled`）；没有取消就不该提前返回。
- **字符串 id**：宿主请求 id 是字符串（`host-N`），响应必须原样回显；`host.call` 的插件侧 id 用字符串或数字均可。
- **迟到响应语义**：被宿主放弃（超时/取消）的 `invoke` 仍占并发槽；全部槽被放弃时宿主杀进程、下一调用重新握手。验收夹具 `nc-undercut.wait ignore_cancel` 用于复现两条路径。
- **describe 必须与安装清单逐字节一致**（语义化 JSON 比较）：实现直接把 `plugin.json` 中的 `descriptor`/`actions` 原样回显，避免默认值重序列化不一致。
- **退役命名空间保留**：`undercut.*`、`nc.*` 等被移除内置的动作命名空间已保留（`retired_identity`），外部插件不能复用旧 action id——本插件使用 `nc-undercut.*`。`provides` 服务 id 同理。
- **document_input 未覆盖**：批量输入（`sides.*.rows` 结构）不映射到通用文档字段，`ui_schema.document_input` 首版未使用；复杂输入以 JSON 直填或后续自定义表单承载。
- **工件只有一个通道**：v1 把单侧全部行拼接为单通道程序文本；多通道/WAITM 同步未回归（退役实现中的双通道同步粗车不在本插件范围）。
