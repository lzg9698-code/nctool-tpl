# 独立项目接入 NCtool：插件开发规范

本规范供另一个独立项目的开发者直接使用。基线为本仓库 2026-10-24 的源码（`cba817c`），外部协议 **2**；插件自身版本、服务版本与宿主版本分别管理。源码核对位置见文末。

文中“必须”为当前接口要求；“建议”为项目组织和可靠性设计建议；“可选”为按业务需要接入的能力。本文不承诺尚未实现的宿主扩展。

## 1. 接入方式与职责

**建议采用外部进程插件，独立仓库开发和发行。** 语言不限，只要能读写标准输入输出并处理 JSON。无需把业务代码加入 NCtool workspace，也无需重写成 Rust。

```text
NCtool CLI / WebUI
       ↕ 动作输入、结果、诊断
NCtool runtime
       ↕ stdin / stdout：双向 JSON-RPC 2.0，每行一条消息
独立项目的插件适配层
       ↕ 进程内函数或项目自己的受控接口
独立项目的业务核心
```

| 责任 | NCtool 宿主 | 独立项目 |
| --- | --- | --- |
| 插件管理 | 本地目录安装、配置、显式启用、启动、协议检查 | 提供插件包、运行依赖及兼容说明 |
| 动作 | 注册、Schema 校验、分发 | 定义输入输出、业务校验和计算 |
| 界面 | 插件面板、通用 Schema 表单、任务与结果展示 | 提供动作及面板描述 |
| 业务数据 | 通用模板及已注册服务 | 自己的模型、存储、数据版本及迁移 |
| 运行控制 | 并发额度、超时、取消通知和故障恢复 | 协作取消、线程安全、资源清理 |

建议将业务核心与适配层分开，保留独立项目原有的独立运行能力。计算函数尽量采用“显式输入 → 结果”，避免依赖宿主内部目录、全局变量或另一插件的实现文件。

**当前能力边界：**

- 外部插件可以贡献动作、版本化服务、Schema 表单面板和资产集合。
- 宿主不加载第三方 JavaScript、React 页面、iframe 或插件自定义前端包；填写 `panel.view` 不会安装新视图。复杂编辑器、三维视图或专用交互需要另外设计宿主扩展，或继续由独立项目自己的界面提供。
- 外部插件不能通过当前进程协议注册 Rust `RenderExtension` 过滤器或 `EventListener`。
- `Artifact` 当前只有 `name`、`media_type`、`text`，没有文件路径、二进制流或 base64 下载字段。PDF、图片、Excel 等原生二进制产物需要另行设计交付接口，不能仅更换 MIME 类型就当作已支持。
- 插件与宿主以同一用户权限运行；进程隔离不是操作系统沙箱。

## 2. 插件包和运行环境

最小 Python 包目录：

```text
acme-tool/
  plugin.json
  plugin.py
  nctool_plugin.py     # 从本仓库 sdk/python/ 复制并随包固定版本
  core/               # 可选：独立项目的业务实现
  README.md           # 建议：安装、依赖、数据目录、支持平台
```

其他语言可改成自己的入口程序。宿主读取包根目录的 `plugin.json`，将本地目录复制到 `<home>/plugins/<插件ID>/`；安装本身不执行插件。安装后的工作目录为插件安装目录，`command` 按 argv 启动，不经 shell。

必须随包提供运行所需文件，或在 README 中说明预先安装的运行时、版本及寻找方式。宿主不会自动运行 pip/npm、创建虚拟环境或安装系统依赖。不要打包 `.git`、缓存、用户资产和凭据；包内不允许符号链接或特殊文件。

| 限制 | 当前范围 |
| --- | --- |
| 包大小 / 文件数 | 最多 64 MiB / 2048 个文件 |
| 描述文件 | 普通文件，最大 1 MiB |
| `timeout_ms` | 10–3,600,000；默认 30,000 |
| `max_message_bytes` | 1024–16,777,216；默认 1,048,576 |
| `max_concurrency` | 1–64；默认 8 |

消息大小限制包含整条 RPC JSON，包括产物文本与转义开销。宿主其他入口还有独立限制，例如 CLI `run --input` 最大 1 MiB；提高 RPC 上限不会同步提高其他上限。

同 ID 已安装目录不会覆盖更新。升级前停止使用该插件的宿主、保存业务数据，再卸载旧包、安装新包、重新启用并启动。不要把唯一业务数据放在安装目录，卸载会删除这个目录。原生程序需针对支持的操作系统和架构分别准备；含运行时的大包超过上述上限时，应改为预装依赖或另行讨论分发方式。

## 3. 最小描述文件

下面是可复制的完整 `plugin.json`。`acme-tool` 是示例标识，实际项目应统一替换命名空间。

```json
{
  "descriptor": {
    "id": "acme-tool",
    "version": "0.1.0",
    "protocol_version": 2,
    "requires": [],
    "provides": [
      {"id": "acme-tool.echo", "version": 1, "action": "acme-tool.echo"}
    ],
    "config_schema": {"type": "object", "additionalProperties": false},
    "panels": [
      {"id": "acme-tool-main", "title": "独立项目", "actions": ["acme-tool.echo"], "view": "form"}
    ]
  },
  "command": ["python", "plugin.py"],
  "actions": [
    {
      "id": "acme-tool.echo",
      "title": "生成文本",
      "input_schema": {
        "type": "object",
        "properties": {"text": {"type": "string", "minLength": 1, "maxLength": 10000}},
        "required": ["text"],
        "additionalProperties": false
      },
      "output_schema": {
        "type": "object",
        "properties": {"text": {"type": "string"}},
        "required": ["text"],
        "additionalProperties": false
      },
      "example": {"text": "来自独立项目的结果"}
    }
  ],
  "limits": {"timeout_ms": 30000, "max_message_bytes": 1048576, "max_concurrency": 1}
}
```

Windows 上建议把 `python` 改为实际解释器的绝对路径，或确保它在宿主启动环境的 PATH 中可用。使用绝对路径的包需要针对目标机器调整；路径与参数是不同 argv 元素，不在元素中额外加 shell 引号。

字段约束：

- ID 以 ASCII 字母或数字开头，只含 ASCII 字母、数字、`.`、`_`、`-`，最多 128 字符；插件版本为 SemVer，协议必须是 `2`，服务版本为正整数。
- 动作 ID 在宿主动作目录中必须唯一；建议所有动作、服务和集合均使用自己的命名空间。服务 `provides[].action` 和面板 `actions[]` 必须指向本插件声明的动作。
- `requires` 列出实际调用的服务及精确整数版本；它不是包管理器依赖列表。多个插件提供相同服务时，通过宿主 `providers` 明确选择提供方。
- 输入、输出、配置都提供 JSON Schema；只允许本地 `#...` 引用。`output_schema` 校验的是 `ActionResult.data`，不是整个 RPC 响应。描述及结果类型拒绝未声明的结构字段，不自行加入 `entrypoint`、`permissions` 等顶层键。
- 不使用内置插件 ID `template`、`parameters`、`math`。已退役插件 ID `nc`、`nc-parameters`、`nc-managed`、`sync-turning`、`undercut`、`process`、`process-parameters` 保留，不能占用。
- 动作与服务不能使用 `nc.`、`machine.`、`process.`、`sync-turning.`、`undercut.` 前缀；旧集合 ID `nc`、`nc-parameters`、`sync-turning-assets`、`undercut-assets` 也保留。新领域项目使用新命名空间，例如 `acme-tool.generate`。

## 4. Python 最小入口

将下面保存为 `plugin.py`，并从 [Python SDK](../sdk/python/nctool_plugin.py) 复制 `nctool_plugin.py` 到同目录。示例只演示适配层；业务实现放到独立模块中调用。

```python
import json
from pathlib import Path
from nctool_plugin import PluginError, PluginServer


def invoke(action, input_data, server, request_id):
    if action != "acme-tool.echo":
        raise PluginError({"code": "action_not_found", "message": "未知动作", "diagnostics": []})
    if request_id in server.cancelled:
        raise PluginError({"code": "cancelled", "message": "请求已取消", "diagnostics": []})
    text = input_data["text"]
    return {
        "data": {"text": text},
        "artifacts": [{"name": "result.txt", "media_type": "text/plain", "text": text}],
        "diagnostics": []
    }


if __name__ == "__main__":
    manifest = json.loads(Path(__file__).with_name("plugin.json").read_text(encoding="utf-8"))
    PluginServer(manifest, invoke).run()
```

SDK 的接收循环与动作线程池分离，可在等待宿主回调时继续处理消息。当前 SDK 线程池固定为 8 个线程；提高描述中的并发值不会扩大线程池。示例使用并发 1，真实项目确认线程安全后再提高。

长任务必须在循环、阶段切换以及产物交付前检查 `request_id in server.cancelled`，协作退出并清理自己的临时资源。单次检查仅适合本例短任务。SDK 不会替业务函数中止任意阻塞调用，也不会自动回滚外部系统写入。

## 5. 跨语言通信与生命周期

协议是 **JSON-RPC 2.0 + UTF-8 换行分帧**。一行一个完整 JSON 对象，及时 flush；标准输出不得混入日志、进度、banner 或业务子进程输出，日志写 stderr。多线程写 stdout 必须串行化；不能使用多行美化 JSON。

宿主依次发送：

| 方法 | 参数 | 插件返回 |
| --- | --- | --- |
| `initialize` | `{protocol_version:2,plugin_id,config}` | `{protocol_version:2,plugin_id}` |
| `describe` | 宿主请求 | `{descriptor,actions}`，与安装包声明按缺省值规范化后完全一致 |
| `invoke` | `{action,input}` | `ActionResult` |
| `cancel` | 通知：`{id:"host-N"}` | 无 RPC 响应；终止对应业务任务 |
| `shutdown` | 通知：`{}` | 无 RPC 响应；释放资源并退出 |

响应必须回传原请求 ID，不能根据线程完成顺序猜测 ID。自研适配器必须持续读取输入、按 ID 分发响应，并避免在接收线程中同步等待宿主回调。

业务错误使用下面的结构，不用成功结果伪装错误：

```json
{
  "jsonrpc": "2.0",
  "id": "host-3",
  "error": {
    "code": -32000,
    "message": "输入不满足业务约束",
    "data": {
      "code": "acme_invalid_input",
      "message": "输入不满足业务约束",
      "diagnostics": [{"level": "error", "code": "acme_invalid_input", "message": "请检查 text", "path": "/text"}]
    }
  }
}
```

诊断可附 `template`、`line`、`column`，行列为从 1 开始的位置；`path` 表示 JSON 字段位置。不要把密钥、私有配置或不必要的绝对路径写入诊断。Python 转发宿主 `PluginError` 时保留其 `detail`。

取消或超时先结束对应宿主请求并通知插件；迟到请求仍占并发名额，直到返回迟到响应。当前源码在所有并发名额均被已放弃请求占满时终止该进程，协议错误或进程退出也会使传输失败；后续独立调用重新启动并握手。**宿主不会自动重试原业务动作**。不要依赖固定 PID、内存状态或永不重启的常驻连接；取消也不能保证副作用已经撤销。

## 6. 输入、输出与可重执行性

建议每个动作只有一个明确业务目的，例如 `acme-tool.validate`、`acme-tool.calculate`、`acme-tool.generate`；不要强制所有项目实现这些名称，也不要用一个任意命令执行接口代替业务 API。

- 输入显式表达单位、坐标/基准、枚举、范围、默认值与可空规则。Schema 负责结构，插件负责跨字段和领域约束；校验失败不得静默修正成另一业务输入。
- 宿主整数字面量范围为 `[-2^63, 2^64-1]`，小数/指数使用有限 f64，不承诺任意精度；拒绝 NaN、Infinity、溢出以及非零数下溢为零。超出安全表达范围的标识或精确十进制建议使用有明确语义的字符串。JavaScript 实现要特别处理超过安全整数范围的输入。
- 成功统一返回 `{data,artifacts,diagnostics}`。正文建议放 `data.text`，供 CLI `--out` 使用；多文件以 `artifacts` 返回，使用简单且唯一的文件名，避免路径段和保留名，便于宿主安全导出。
- 尽量在完整计算和校验成功后返回产物。失败、取消后不得返回可误认为有效成品的部分结果。
- 有副作用的动作建议自行定义幂等键、重复提交规则和事务/补偿策略。幂等键是业务字段，宿主不自动提供幂等执行保证。
- 宿主不会自动把任意业务文件、数据库或远程资源冻结到历史。需要可重执行时，插件应固定实际使用的数据和版本，必要时返回 `data.replay_input`；该值必须满足同一动作的输入 Schema。
- 历史环境比较不保证结果完全相同；快照也不固定插件程序和包外依赖。业务算法版本、模型/规则版本等由插件明确记录。

## 7. 可选：调用宿主服务

无需模板渲染的独立项目可以保持 `requires:[]`。需要渲染时声明 `{"id":"template.render","version":1}`，然后在动作中调用：

```python
rendered = server.call(request_id, "template.render", {
    "source": "结果：{{ value }}",
    "context": {"value": "业务结果"}
}, version=1)
# rendered 是 ActionResult，正文在 rendered["data"]["text"]。
```

底层发送 `host.call`，params 为 `{parent_id,service,version,input}`；`parent_id` 必须是仍有效的宿主动作请求 ID。只能调用 `requires` 声明且版本匹配的服务。循环动作链被拒绝，嵌套深度最多 32。

若“检查模板 → 领域校验 → 渲染”跨多个调用，使用 `template.inspect` 的 `include_snapshot:true` 得到快照，再把同一快照传给 `template.render`，避免两次读取期间资产改变；对应服务都要在 requires 中声明。快照完整字段以 SDK 的 `TemplateSnapshot` 为准，按服务原样传递，不能自行丢弃其版本或元数据。数学扩展另需编译、启用并在渲染输入 `extensions` 中显式选择。

## 8. 可选：接入模板工作台

有普通动作面板即可交付的项目，无需此项。若希望出现在模板的“生成方式”中，可在动作上声明：

```json
{
  "ui_schema": {
    "document_input": {
      "title": "使用独立项目生成",
      "match_metadata": "acme-tool",
      "bindings": {
        "source": "source",
        "template": "id",
        "context": "effective_context",
        "options": "metadata.acme-tool"
      }
    }
  }
}
```

这是动作描述的可选片段，**不能直接加入第 3 节 echo 动作**：实际动作的 input_schema 必须声明并允许相应 source/template/context/options 字段。bindings 的键是动作输入字段，值是宿主文档数据来源；不存在的值不发送。

资产可使用自己的 `metadata["acme-tool"]` 存领域选项，并明确设置 `metadata.execution.action` 为自己的动作。`effective_context` 递归合并模板默认值和当前输入；有共享参数解析需要时，应另行按共享参数服务契约接入，这个合并值不等于完整的参数解析/冻结结果。

不要使用旧 `metadata.nc` 或退役执行动作标记，当前宿主会拒绝其作为普通/受管模板执行。插件未启用、服务缺失或动作不可用时，应明确失败；不要设计成偷偷改用普通文本生成。适配时验证当前 WebUI 实际选择行为，不依赖加载顺序自动选择提供方。

## 9. 可选：配置、数据和资产集合

`initialize.config` 来自 `<home>/config.json` 中 `plugins[插件ID]`，按 config_schema 校验；配置与启用状态在下次宿主启动时生效。建议只放长期选项，单次业务数据放动作输入。

当前握手不注入通用 workspace 路径，也没有通用文件系统 CRUD 回调；不要假设进程工作目录就是用户 workspace。自有数据目录需由插件配置或动作契约显式确定，校验路径边界，并自行保证冲突检测、原子写入和备份。不得绕过宿主资产服务直接覆盖模板或共享参数文件；Rust 侧宿主资产写入继续使用 JsonStore/WriteKernel。

需要参加工作区资产包导入导出时，在 descriptor 声明：

```json
{
  "asset_collections": [
    {"id": "acme-tool-assets", "export_action": "acme-tool.assets.export", "import_action": "acme-tool.assets.import"}
  ]
}
```

两个动作必须同时声明自己的 Schema。导出动作接受 `{}`，返回集合数据；导入动作支持 `{collection,validate_only:true}` 纯预检查并返回 count/mapping，正式导入返回 receipt，回滚接受 `{rollback:receipt}`。这些都是动作输入/`ActionResult.data` 内的数据，不能替代外层 ActionResult。

预检查不得写入；receipt 必须足以只撤销本次新增内容。导入和回滚都要处理并发冲突，不覆盖既有资产或删除后续用户修改。集合 ID 必须全局唯一，跨插件导入不提供统一数据库事务。可执行细节和内置参考见 [bundle 编排实现](../cli/src/composition.rs)及[参数插件](../plugins/parameters/src/lib.rs)。

## 10. 安装与首次联调

以下 PowerShell 命令假定 `nctool` 已在 PATH、当前目录放置 `acme-tool` 插件包，且包的解释器命令可运行。使用全新的测试目录；不指向真实用户资产：

```powershell
$pluginTestRoot = Join-Path ([IO.Path]::GetTempPath()) ("nctool-plugin-" + [guid]::NewGuid())
$pluginTestHome = Join-Path $pluginTestRoot "home"
$pluginTestWorkspace = Join-Path $pluginTestRoot "workspace"
New-Item -ItemType Directory -Path $pluginTestWorkspace -Force | Out-Null
nctool --home $pluginTestHome --workspace $pluginTestWorkspace plugins install ./acme-tool
nctool --home $pluginTestHome --workspace $pluginTestWorkspace plugins enable acme-tool
nctool --home $pluginTestHome --workspace $pluginTestWorkspace actions
'{"text":"hello"}' | nctool --home $pluginTestHome --workspace $pluginTestWorkspace run acme-tool.echo --input -
nctool --home $pluginTestHome --workspace $pluginTestWorkspace ui --port 0
```

每一步检查退出码。安装结果为未启用；下一次 CLI 启动会读取 enable 保存的配置。WebUI 常驻进程需要重启。最后一条命令启动前台服务，按终端实际输出的随机端口访问并检查插件面板；完成后停止本轮进程。

CLI `run` 直接输出 ActionResult。HTTP 成功响应另包一层 `{ok:true,data:...}`，不要混用解包方式。CLI `--out` 写正文，`--out-dir` 将全部文本 artifacts 交付到一个新目录；详细行为见[系统设计](SYSTEM_DESIGN.md)。

## 11. 交付与验收要求

独立项目首版建议按“一个真实业务动作 → 宿主 CLI 调通 → Schema 面板 → 可选模板集成/资产集合”推进。

交付物至少包含：插件目录或各平台安装包、完整 plugin.json、依赖与启动说明、动作输入输出示例、业务数据格式/单位说明、兼容版本说明、业务测试和插件协议验收报告。有存储时另附数据迁移、备份与升级步骤。

| 验收范围 | 必须检查 |
| --- | --- |
| 安装与启动 | 包完整、解释器可用、初始化和 describe 一致；安装不启动代码 |
| 正常业务 | 真实业务样例、输出 Schema、文本产物及诊断 |
| 非法输入 | 缺失、类型、范围、未知字段、跨字段约束、数值精度边界 |
| 服务依赖 | requires 与真实调用一致，缺失/版本不符明确失败；没有回调则标不适用 |
| 并发与取消 | 请求不串线、取消及时、兄弟请求可继续、超时及迟到响应 |
| 故障恢复 | 协议错误、异常退出、全部槽位被放弃请求占满后的重新握手 |
| 副作用与重执行 | 失败不冒充成功、重复调用行为明确、快照完整、数据不丢失 |
| 界面与安装环境 | 实际表单能提交；所声明支持的平台分别验证 |

协议自动验收使用本仓库现有工具：

```powershell
python scripts/check_plugin.py --binary <固定宿主路径> --source <测试插件目录> --plan <测试计划.json> --python <Python解释器路径> --output target/acme-tool-acceptance.json
```

尖括号为需要替换的占位符；`--python` 适用于工具支持的 Python 入口替换，其他语言省略。测试计划需按[插件作者验收](PLUGIN_ACCEPTANCE.md)准备成功、非法输入、取消、超时、忽略取消等测试路径。最小 echo 示例没有这些故障注入动作，不能直接冒充完整协议验收插件。可参考 [protocol-probe](../examples/plugins/protocol-probe) 制作测试构建，避免将测试专用控制字段暴露给正式业务。

报告必须注明宿主二进制 SHA-256、插件版本、测试计划与平台；未执行项写明未验证。协议通过不代表业务算法正确，领域业务还需自己的验证标准；涉及设备执行的输出另做设备/OEM、仿真与现场验收。

## 12. 源码依据与参考

| 内容 | 当前依据 |
| --- | --- |
| 类型、诊断、ActionResult、Schema、快照 | [sdk/src/lib.rs](../sdk/src/lib.rs) |
| 描述、限额、握手、双向 RPC、取消及恢复 | [runtime/src/external.rs](../runtime/src/external.rs) |
| 安装、配置、保留 ID | [runtime/src/config.rs](../runtime/src/config.rs) |
| 描述与注册校验 | [runtime/src/lib.rs](../runtime/src/lib.rs) |
| CLI 命令与产物交付 | [cli/src/main.rs](../cli/src/main.rs) |
| 文档动作输入绑定 | [ui/src/lib/execution.ts](../ui/src/lib/execution.ts) |
| 跨语言适配参考 | [Python SDK](../sdk/python/nctool_plugin.py)、[报告插件](../examples/plugins/python-report) |
| 完整协议与跨语言契约 | [插件协议](PLUGIN_PROTOCOL.md)、[宿主与插件契约](CONTRACTS.md) |

遇到旧文档对取消行为的不同描述，以当前运行时源码为准：单个请求超时/取消不会无条件终止仍服务其他请求的进程。本规范与旧文档不能作为未实现功能的授权或兼容保证。
