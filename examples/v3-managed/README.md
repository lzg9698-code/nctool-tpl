# V3 受管参数迁移资产包

本目录是可导入的工作区包，不是自动装入用户工作区的内置资产。28 份模板包含 26 份业务模板和 2 份键槽公共片段。正式生成使用 Rust，不调用 Python，也不读取 V3 项目。

| 文件 | 用途 |
| --- | --- |
| [workspace.json](workspace.json) | 模板、128 项共享定义、刀具/材料/加工预置及 ES/FS 标准集合 |
| [mapping.json](mapping.json) | 每份模板的稳定 ID、源哈希、原始绑定、变量规则、Python 辅助函数、模板局部公式、引用和语法改写 |
| [classification.json](classification.json) | 排除的历史程序文本、共享默认值策略与缺失事实 |
| [source-hashes.json](source-hashes.json) | 冻结副本 core/configs/templates 的逐文件 SHA-256 |
| [oracle.json](oracle.json) | 冻结 Python 生成器产生的中间上下文、分支、同步编号和程序对照样本 |
| [direct-inputs.json](direct-inputs.json) | 普通模板的本次输入及主程序的当前任务依赖示例 |
| [validation.json](validation.json) | 本轮逐模板三层验收证据、构建指纹及测试结果 |

用 NC 构建启动隔离工作区：

```powershell
cargo build -p nctool-cli --features nc-bundle --locked --target-dir target/managed-nc
target/managed-nc/debug/nctool.exe --home target/demo-home --workspace target/demo-workspace --profile nc ui --port 0
```

在“工作区迁移”中选择 workspace.json，完成预检查后导入。导入拒绝同名冲突，集合失败回滚；不要直接覆盖运行目录。JSON 数字字面量参与包指纹，使用工作台导入器或保持原始 JSON 字节发送，避免用普通 JavaScript JSON.parse/stringify 重写资产包。

模板库打开迁移模板时会选择 `nc.generate.managed`；表单显示原始工艺输入。普通多工序编辑页也使用后端处理器。双通道分配和主程序依赖使用“受管工序与组合程序”，在 ops 的 result_bindings 引用本次前置工序，outputs 分配独立 MPF/SPF。主程序不能把历史变量库中的程序文本当作输入。

同步车请求文件示例：

```json
{"template":"v3-sync-turning-right-template","operation_inputs":{"sta_x":60,"sta_z":0,"end_x":50,"end_z":-10,"ap_1":2,"ap_2":3,"sync_feed":0.2,"current_sync_num":10}}
```

```powershell
target/managed-nc/debug/nctool.exe --home target/demo-home --workspace target/demo-workspace --profile nc run nc.generate.managed --input examples/v3-managed/requests/sync-raw.json --out-dir target/new-sync-files
```

完整接口及加工预置格式见 [参数管理指南](../../docs/PARAMETER_MANAGEMENT.md)。迁移的刀具事实与经验转速/进给分别保存；源项目缺少的齿数、补偿号和材料牌号明确待补。零件尺寸和临时输入不保存为共享默认，示例输入仅用于验收。材料 `v3-unspecified` 表示源资料缺失，不代表已经确认了适用材料。

[requests](requests) 提供可运行的键槽、同步车、卡簧槽原始输入，以及双通道输出和当前工序绑定的主程序示例。主程序请求使用 `process.generate.managed`。

开发时重新生成冻结副本和资产：

```powershell
python scripts/migrate_v3_parameters.py --source D:/Desktop/nctool/NCtool-V3.0
python scripts/verify_managed_parameters.py --binary target/managed-nc/debug/nctool.exe
node scripts/browser_managed_parameters.cjs
```

开发对照需要 Python、Jinja2、PyYAML；浏览器脚本使用 NCTOOL_BINARY/NCTOOL_PLAYWRIGHT/NCTOOL_CHROMIUM 配置。先运行 CLI/HTTP 对照，它会把本次生成片段的组合任务补入 direct-inputs.json，然后运行浏览器。迁移脚本不运行 V3 的写入测试，不更新源项目。冻结副本与临时验收目录位于忽略的 target/parameters-validation；复现时请保留源哈希一致性。

主程序里的 FINISH_TURNING 和 left_T_START3_END3 在 V3 变量库中是手写或历史轮廓文本，没有相应 Python 计算器。验收为这些槽位显式创建了带尺寸/终点/进给原始输入的新工序，并绑定本次输出；资产包不会猜测实际零件轮廓，也不会自动复用旧文本。实际使用需要选择自己的轮廓工序。

输出对照仅允许预先声明的 LF、每行首尾空白、末尾换行适配。指令、数值、通道、片段顺序和同步编号必须一致。公共片段通过全部依赖它的键槽阶段验收。`v3_fixed` / `v3_signed` 保留 Python 格式化的有符号零和三位舍入；原有严格 NC 过滤器保持原行为。

这份包保留 V3 来源与未审核状态。软件迁移验收结果见 [实际验证记录](../../docs/archive/records/MANAGED_PARAMETERS_2026-10-07.md)，技术验收不等于目标控制器和机床工艺审核。
