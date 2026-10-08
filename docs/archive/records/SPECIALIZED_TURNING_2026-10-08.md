# 同步粗车与越程槽专用插件实施及验证

2026-10-08，Windows x86_64；基于 HEAD `f0fda2d4e888e70fa5b0726ec77fd7088b6e95d3` 的未提交工作树。现有改动保留，没有提交、推送或发布。版本仍为 CLI/WebUI 2.2、组件 2.0。使用与接口见[专用插件指南](../../SPECIALIZED_TURNING_PLUGINS.md)。

## 实施范围与基线

- 新增 `sync-turning`、`undercut` 内置插件与独立页面。同步粗车提供多行推荐/采用、左右独立及四通道生成；越程槽提供 ES/FS 联动、34 项标准和左右批量。
- Rust 完成原始输入校验、计算、同步推进和类型化轨迹；Python 仅作开发对照。通用参数库与根引擎没有加入 NC 几何计算。
- 拆分 NC 基础服务和兼容分派。旧单条受管 API 保留，组合支持业务批次、当前通道绑定、连续同步号及全任务原子交付。
- 第一版轨迹资产提供受约束策略参数、临时调整确认、恢复、复制/保存、导入导出、指纹冲突和旧快照重执行。程序和名义坐标预览共用计划。
- 更新三份迁移模板为 v1 包装，保留转换前源及来源哈希；用户已安装模板没有自动覆盖。提供[28 模板包](../../../examples/v3-managed/workspace.json)和[专用会话/轨迹包](../../../examples/specialized-turning/workspace.json)。
- 开始时 421 文件恢复基线：`.nctool-backups/turning-20261008`，归档 SHA-256 `73b42fa5e512e2c125cbeb3ef8967ebe26402d96d0e1e939347ca770df90d60c`。本轮按此基线区分原有改动，未将仓库总 diff 认作本轮修改。
- V3 对照副本在 `target/turning-validation/frozen-v3`，88 个文件逐项冻结并核对当前源目录 SHA-256，保持一致。未更新 V3 源目录或原对照快照。
- 默认/NC target 分离为 `target/turning-template`、`target/turning-nc`。验收使用临时 home/workspace、空闲 loopback 端口和本轮创建的浏览器，未使用用户运行资产。
- 最终验收冻结二进制 `target/turning-validation/nctool-final2.exe`，SHA-256 `07d83a1dac0f6196a0492b09248e11ffbde8db9c87e2249fd7591a09f0375e0f`。
- 原 `cli/ui/source.sha256` 为 `330f14b0123db1aefdb410dd33b5ff5b79d2a0a5139c64eb8a33b924e051f20f`，最终为 `c5932ea8db68dc5209ff899c78322c160d57a267d75e3e5f17ef75ba9ea079fd`，差异来自正常前端构建。

## 本轮检查

| 检查 | 实际结果 / 本地证据 |
| --- | --- |
| Rust workspace 全 target/features | 457 通过；`target/turning-rust-tests.log` |
| 专用插件集成测试 | 12 通过；`target/turning-specialized-tests.log` |
| Rust 文档测试 | 4 通过；`target/turning-doc-tests.log` |
| 前端单元测试 | 24 通过；`target/turning-ui-tests.log` |
| Python 验证工具测试 | 14 通过；`target/turning-tool-tests.log` |
| 冻结 Python/Rust 专用对照 | 115 通过：39 推荐、76 计算/程序；`target/turning-validation/python-rust.json` |
| 全模板 CLI/HTTP 回归 | 142 通过，0 失败；`target/turning-validation/managed/report.json` |
| 全模板 WebUI 回归 | 126 通过，0 页面异常；`target/parameters-validation/browser/report.json` |
| 专用页面浏览器 | 19 场景通过，0 页面异常；`target/turning-validation/browser/report.json` |
| 原有两组浏览器 | `browser_v2.cjs`、`browser_review_fixes.cjs` 通过；`target/turning-browser-v2.log`、`target/turning-browser-fixes.log` |
| 文档中的三份生成输入/轨迹保存 | 实际执行通过；`target/turning-validation/examples.json`；专用资产包实际导入通过 |
| 默认 workspace 检查、NC 构建 | 通过；`target/turning-template-check.log`、`target/turning-build.log` |
| fmt / clippy / 架构 / 生成资源一致性 | 通过；`target/turning-clippy.log`、`target/turning-architecture.log`、`target/turning-ui-check.log` |
| 当前依赖审计 | Rust/npm 门槛通过，锁文件未变；`target/turning-audit-final/summary.json` |

工作区测试中的专用 12 项为同一组，不额外累加。115 专用对照与 142 全模板对照有重叠，也不作为独立总数相加。

专用对照对离散值严格相等，浮点计算同时检查误差和程序精度；程序只允许 CRLF→LF 和一个末尾换行适配，不忽略指令、空格、数值、通道、顺序或同步号。专用 WebUI 样本使用同一严格程序比较；原有其他模板保留原记录的逐行外空白适配。逐模板来源、输入绑定和语法改写见 [mapping.json](../../../examples/v3-managed/mapping.json)，V3 变量与新轨迹上下文分别标记。

覆盖整数/近整数刀次、无需切削、多行游标、左右通道、推荐舍入不可采用、ES/FS 全标准左右变换、原始输入生成和伪造派生字段拒绝。新增验证还包括耦合角同时改变坐标、配对同步与补偿状态、未知策略拒绝、旧模板冻结/冲突/重执行、旧自由模板封闭上下文 Schema、几何变化重新确认、单通道组合连续行号及非法程序输入绑定。

浏览器实际填写原始输入并采用推荐，生成四通道和左右 FS，调整轨迹并确认，复制/导出、缩放/适应范围、加入组合、后续行失败无文件、草稿刷新、模板直接转入、资产包导入和单插件关闭。截图在 `target/turning-validation/browser/`：`sync-success.png`、`undercut-success.png` 及 1440×950、1280×720、853×480、390×844 视口。程序截图等待后台结果真正显示后采集。

本轮发现并修复模板切换不刷新专用输入、无损 JSON 会话版本误判、轨迹版本误判、兼容准备结果数值检查、组合行号遗漏及旧自由模板严格 Schema 接收到新传输字段。全部迁移模板已重新从原始输入通过 CLI、HTTP、WebUI；公共片段通过依赖它们的业务模板覆盖，没有手工补齐计算字段。

## 复现命令

先设 `TEMP/TMP` 为仓库 `target/turning-test-temp`，设 `NCTOOL_BINARY` 为固定 NC 二进制；Playwright/Chromium 使用独立安装目录，通过 `NCTOOL_PLAYWRIGHT/NCTOOL_CHROMIUM` 指定。

```powershell
cargo test --workspace --all-targets --all-features --locked --target-dir target/turning-nc
cargo test --workspace --doc --all-features --locked --target-dir target/turning-nc
cargo check --workspace --locked --target-dir target/turning-template
cargo clippy --workspace --all-targets --all-features --locked --target-dir target/turning-nc -- -D warnings
cargo fmt --all --check
npm run --prefix ui test
node scripts/build_ui.mjs --check
python scripts/check_v2.py
python -m unittest discover -s scripts/tests -p 'test_*.py'
python scripts/verify_specialized_turning.py --binary target/turning-validation/nctool-final2.exe
python scripts/verify_managed_parameters.py --binary target/turning-validation/nctool-final2.exe --report target/turning-validation/managed
node scripts/browser_specialized_turning.cjs
node scripts/browser_managed_parameters.cjs
node scripts/browser_v2.cjs
node scripts/browser_review_fixes.cjs
python scripts/audit_dependencies.py --output target/turning-audit-final
```

Windows 沙箱曾阻止 Chromium 启动及 loopback HTTP 连接，在自动批准的隔离运行中完成这些检查。首次 RustSec 网络获取失败的报告仍保留在 `target/turning-audit-online`，最终重试成功。RustSec 当前库提交为 `b8a1a33e246a0a9a3b5f377248c41a503defec74`：0 漏洞，有既有 `yoke-derive 0.8.3` 撤回版本提示；npm 为 0 漏洞。没有升级依赖、忽略公告或修改锁文件消除提示。

## 交付边界

当前完成专用输入和 Rust 计算迁移、受约束轨迹参数调整。任意图形节点编辑、新轨迹策略、旧自由模板程序解析预览属于后续扩展，未宣称完成。2D 是名义坐标示意，未进行 ANG/RND、真实刀补、碰撞或机床仿真。进给延续 V3 的程序模式约定，仍需与目标机床模式核对。

本轮只在 Windows 本地验收，未跑远端 CI、其他操作系统或真实机床。本地日志/截图位于忽略的 target 目录，不保证随仓库分发；源码、文档、资产包、映射和对照样本可随工作树交付。
