# Rust 工作台收束为通用模板：实施与验证

日期：2026-10-14。平台：Windows 11。基准HEAD `e8fafd5e47a032d52f9f7a6adfbb384d454e0e15`，实施针对含既有未提交/未跟踪内容的本地工作树。没有暂存、提交、推送或发布。

## 范围

按[精简方案](../../RUST_WORKBENCH_SIMPLIFICATION_PLAN.md)把Rust工作台收束为通用Jinja模板产品。移除四个加工crate及它们的动作、服务、页面、资产模型和构建分支；保留根引擎、资产、SDK、运行时、模板插件、共享参数插件和数学扩展（独立feature）。独立 `shaft_programmer` 与其数据、旧工作台归档、研究原型均未改动。

## 实现要点

- Cargo workspace与CLI依赖移除 `nc`/`process`/`sync-turning`/`undercut`；`nc-bundle` 退出，数学改为独立可选feature（默认不编译、运行需显式启用）。CLI composition、descriptor、inventory、CLI enable/install、运行时config统一收束内置插件名单。
- 退役标识保留为保留名：旧加工插件ID、动作前缀、集合ID和nc profile在配置、安装、动作执行、集合导入各入口拒绝，且阻止同名外部插件接管旧enabled配置。
- 旧配置提供只读迁移预览（`config check`，HTTP `GET/POST /api/v2/config/migration/check`）与显式应用（`config migrate --expected <指纹>`，HTTP `POST /api/v2/config/migration/apply`）。命令强制显式 `--home`；指纹陈旧先拒绝、不备份不写入；应用前备份原字节并做指纹守卫；保留未知外部插件与provider并做依赖预检。
- 旧加工意图边界：模板 `metadata.nc` 字段存在（含空对象/null）或 `metadata.execution.action` 属退役前缀时，模板可读可编辑，但 `template.render` 与 `template.render.managed` 在CLI/HTTP/快照各入口拒绝，不静默降级为普通文本；任意其他metadata不视为领域意图。
- `template.render.managed` 与 `template.render` 增加显式 `extensions` 输入并冻结进 replay_input，使共享参数渲染可请求数学扩展；不默认注入。
- 旧加工历史保留原artifact、可查看/比较/导出；重执行与恢复为执行文档被拒绝并给出原因，原记录字节不变。混合加工资产包在整体导入前拒绝，不部分写入。
- 前端主导航收为模板、共享参数、历史、设置；删除NC/机床/多工序/专用车削页面、状态、资源请求和域CSS，保留通用表单、任务详情、历史比较、参数依赖订阅、外部插件页与工作区导入导出。启动器改为 `--features math --profile template`。
- CI/release改为通用构建；独立轴类与通用Python工具拆为独立job，保留各自门槛；架构检查改为通用依赖与独立math边界，校验退役源码/依赖/chunk不存在。

## 实际验证（Windows 11）

| 检查 | 结果 |
| --- | --- |
| `cargo test --workspace --all-targets --all-features --locked`（`target/rust-template-simplification/rust-math`） | 253项通过，0失败，19个套件全ok |
| `cargo test --workspace --all-features --doc --locked` | 4项通过 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 通过 |
| `cargo fmt --all -- --check` | 通过 |
| `rustup run 1.89 cargo check --workspace --all-features --locked` | MSRV 1.89通过 |
| `cargo build -p nctool-cli --no-default-features --locked` | 默认组合构建通过 |
| `cargo build -p nctool-cli --features math --locked` | 数学组合构建通过 |
| `npm run --prefix ui test` | 7文件、37项通过 |
| `npx tsc --noEmit` | 通过 |
| `node scripts/build_ui.mjs` | 构建通过，嵌入资源6文件一致，无退役chunk |
| `python scripts/check_v2.py` | 通过（纯模板依赖树、独立math、退役源码/依赖/chunk移除、包内容） |
| `node scripts/browser_v2.cjs`（固定binary、临时home/workspace、随机端口） | 通过 |
| `node scripts/browser_review_fixes.cjs` | 通过 |
| `python -m unittest discover -s scripts/tests -p 'test_*.py'` | 101项通过 |
| `python scripts/audit_dependencies.py` | 退出0；无漏洞命中，保留unsafe-libyaml unmaintained与yoke-derive yanked告警；锁文件未改变 |
| 归档核对 `test_rust_nc_archive.py` | 205文件逐字节一致；新临时目录恢复一致 |
| 保护范围核对 | 独立轴类、研究、旧历史、旧工作台归档、用户循环源码、迁移工具等125文件字节未变 |

浏览器验收首次失败3处：能力字段访问用了旧嵌套结构 `item.action.id`，以及布尔渲染预期写成 `true`/sqrt预期写成 `4`。核对后端实际行为（MiniJinja布尔为 `True`、math `sqrt` 返回 `4.0`，均非本轮回归）后修正脚本断言，重跑通过。失败日志与截图保留在 `target/rust-template-simplification/browser-*/`。

UI来源指纹由 `412f2cc4df6785fd3e64d92bffb23852bdfd29a36b0abebda9295febe48ab1a3` 变为 `902f34974e630cbfb6fda0e0fb9aad3637763ca335017db20ba442bd82594c23`，由正式构建生成，未手改。

## 未执行与遗留

- 未运行Linux/macOS、真实远端CI、发行二进制或依赖升级；无MSRV以外平台证据，也未准备发布。Cargo依赖变更已审计并保留报告。
- 归档模块的退役删除动作被自动审批拒绝（不可逆本地删除），改为校验后移动到忽略的恢复目录，未重试删除或绕过权限。恢复副本 `target/rust-template-simplification/retired-backend-files`、`retired-script-files` 保留。
- 13处历史归档记录中的失效链接指向已退役源码/示例，已按归档规范加退役说明并保留原文，不改写引用。
- 软件回归不代表加工工艺等价或机床验收；独立轴类产品的能力与验收边界不变。

恢复基线：`target/rust-template-simplification/baseline`（429文件，含原Git状态与二进制补丁）。加工模块归档：[RUST_NC_RETIREMENT](../legacy/RUST_NC_RETIREMENT.md)（205文件，SHA-256 `8837a6fa9bb1167dc8558c45bd6f25a2fc9b16390f59aca5d27adf1fd30ace35`）。
