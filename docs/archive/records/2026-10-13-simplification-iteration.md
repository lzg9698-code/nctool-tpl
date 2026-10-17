# 项目精简第一轮实施与验证

日期：2026-10-13。平台：Windows 11。基准提交：`a7a507d7529ad8a2f722ca00836bc100b59c0517`，验证针对含既有未提交/未跟踪内容的本地工作树。没有提交、推送或发布。

## 已完成

- 轴类产品主线明确为 `shaft_programmer`；Rust 通用模板平台保持领域中立。旧 `workbench` 保留固定方案兼容入口、数据和历史，尚未迁移或删除。
- Rust 的启动、配置预检查、下次启用清单共用 `selected_builtin_ids`，修正禁用主插件后默认伴随仍显示下次启用的问题。保留显式启用、禁用优先级、编译特性与依赖拒绝语义；删去 NC 插件立即过滤掉的两条动作构造。
- 前端依赖核对按工作区、任务、核对动作及精确输入共享订阅。同一身份一条轮询链，完成后再调度，最后卸载清理，旧响应不能污染新订阅。核对中、失败及依赖变化仍使当前导出失效。
- App 的任务关联只保留 Document.lastRunId/lastRunSignature，删除重复 execution 索引。历史输入恢复的关联进入草稿，副本清除关联；历史记录本身不改写。
- 旧工作台单独 TypeScript 检查及构建，不进入 Rust UI 的来源散列。主 UI 来源散列纳入构建脚本；旧工作台来源清单补充实际配置、锁文件及构建脚本。当前旧页面仅依赖自身业务源码和 npm 包，未来引入共享源码须扩展清单。
- 轴类文档结构与工序参数集中为显式契约，不再由样例/生成器推导。JSON 读取、路径及实现指纹工具移到 common.py；CLI、HTTP 与打包复用。保留旧快照可选字段、可保存待修正草稿、完整失败阻断及独立 NC 读取。
- 轴类浏览器验收先验证未知环槽刀宽阻断，再通过表单填写专用测试事实；检查实际裸子程序调用、通道根文件及独立核对记录，取消固定 12 文件假设。生产样例的未知字段保持空白。
- CI 新增三平台轴类单元测试、页面语法及 Linux 轴类浏览器门槛与证据上传；导航和贡献指南同步。工作流尚未远端运行。

## 实际验证

| 检查 | 当前本地结果 |
| --- | --- |
| `npm run --prefix ui test` | 9 文件、42 项通过，包含 10 项共享订阅边界测试 |
| `node scripts/build_ui.mjs`、`--check` | 构建与 11 个嵌入文件一致性通过 |
| `node scripts/build_workbench.mjs` | 独立类型检查和构建通过；Vite 提示忽略 lucide-react 的 use client 指令，退出码 0 |
| UI 来源边界临时副本试验 | 修改旧入口不影响主指纹；修改主模块正确拒绝 |
| `cargo test --workspace --all-targets --all-features --locked --target-dir target/simplification-nc` | 466 项通过 |
| `cargo test --workspace --all-features --doc --locked --target-dir target/simplification-nc` | 4 项通过 |
| `cargo test --workspace --locked --target-dir target/simplification-template configuration_` | 默认组合 3 项配置回归通过 |
| NC 组合 `configuration_` 及受管冻结重放定向回归 | 分别 4 项及 1 项通过，后续也包含在全量中 |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets --all-features --locked --target-dir target/simplification-nc -- -D warnings` | 通过 |
| `python scripts/check_v2.py` | 纯模板依赖、领域边界、包内容和生成资源检查通过 |
| `python -m unittest discover -s shaft_programmer/tests -p 'test_*.py'` | 276 项通过，使用 PYTHONDONTWRITEBYTECODE=1；改前基线 264 项通过 |
| `node --check shaft_programmer/web/app.js` | 通过 |
| `scripts/browser_v2.cjs`、`scripts/browser_review_fixes.cjs` | 两组通过；固定本轮 NC 二进制、临时 home/workspace 和端口 |
| `shaft_programmer/tests/browser.cjs` | 最终 15 个场景通过，零未捕获异常，不触发下载 |
| 文档链接、`git diff --check` | 通过；Git 提示 docs/README.md 的 CRLF 转换 |

浏览器使用独立 Playwright 安装与已有 Chromium 1208；未添加产品依赖。Rust 默认与 NC 使用不同 target 目录。普通 Rust WebUI 回归包含其既有文件导出检查，轴类验收未触发下载。

原 UI 指纹已备份至 `target/simplification/original-source.sha256`。构建前值 `2dd85c2bd8a68fd8fb0a5aa9fac50ea57d6e26841bda34f76c3876bc0cbc2e20`，最终值 `412f2cc4df6785fd3e64d92bffb23852bdfd29a36b0abebda9295febe48ab1a3`，由正式构建生成。新增来源边界和源码修改解释该差异，不为清理脏状态手改。

本地日志及源码证据在忽略目录 `target/simplification/`：rust-tests.log、rust-doc-tests.log、template-tests.log、clippy.log、architecture.log、ui-build.log、ui-browser.log、fixes-browser.log、shaft-browser/report.json、source-evidence.json。日志不随仓库保证提供。

## 未完成与下一轮

- 旧 workbench 作业迁移、代表性输出核对与退役尚未完成；旧验收不继承到新结果。
- NC 最终处理链、资源按需读取和 CSS 覆盖清理尚未实施。本轮优先减少重复规则、状态和构建耦合，没有用新增框架或批量合并 crate 达成表面缩减。
- GitHub Actions、Linux/macOS、MSRV 1.89、依赖审计与 scripts/tests 全套未在本轮执行；没有依赖或锁文件变更，也没有准备发布。新 CI 门槛是否跨平台通过须由实际远端结果确认。
- 软件回归不代表控制器、真实刀具、夹持或机床工艺验收，输出继续为仿真候选。

执行指引：[审查](../../../.agents/skills/nctool-review/SKILL.md)、[验证](../../../.agents/skills/nctool-verify/SKILL.md)、[文档同步](../../../.agents/skills/nctool-docs-release/SKILL.md)。原审查建议见[精简审查报告](../../PROJECT_SIMPLIFICATION_REVIEW_2026-10-13.md)。
