# 使用 Codex 开发 NCtool

项目规则在 [AGENTS.md](../AGENTS.md)，专用 Skills 位于 `.agents/skills/`，随仓库共享。规则与 Skills 不会自动提交、推送或发布改动。

## 三个专用 Skills

| 调用 | 适用任务 | 预期结果 |
| --- | --- | --- |
| `$nctool-review` | 审查本地 diff 或 GitHub PR、定位回归 | 文件位置、触发条件、影响与证据 |
| `$nctool-verify` | 运行验证、浏览器验收、诊断 CI | 实际命令、平台、报告及未执行项 |
| `$nctool-docs-release` | 文档同步、归档、发布准备 | 对齐实现的文档与发布检查 |

例如：“用 `$nctool-review` 审查当前未提交改动，重点看历史重执行。”或“用 `$nctool-verify` 验证参数编辑修复，使用临时 home/workspace。”技能也可按任务描述自动匹配。新增后如当前会话列表未更新，重新打开项目会话再检查，无需复制到全局技能目录。

## GitHub 与浏览器

GitHub 插件读取仓库、PR diff、检查与 Actions 日志；先核对 `git remote -v`、目标 PR 与提交 SHA。空检查列表不证明成功。连接器未包含所需结果时可用 `gh` 只读补充：

```bash
gh run list --repo lzg9698-code/nctool-tpl --commit <完整提交SHA> --limit 20
gh run view <运行ID> --repo lzg9698-code/nctool-tpl --log-failed
```

远端 CI 只证明对应提交；本地未提交改动仍需验证。查资料不授权发布、合并、评论或更新 issue。

浏览器插件用于手工复现与页面检查。启动固定二进制：

```bash
cargo build -p nctool-cli --features math --locked --target-dir target/codex-math
# Windows 在二进制文件名后加 .exe
target/codex-math/debug/nctool --home <临时home> --workspace <临时workspace> ui --port 0
```

打开启动日志的实际 loopback 地址。`--profile` 仅支持 `template`，旧 `nc` profile 以 `invalid_profile` 拒绝；数学扩展需以 `--features math` 构建（完整二进制已包含），在配置 `enabled` 中启用，并在请求 `extensions` 中显式选择。产品默认端口仍是 8788。不要改用用户工作台测试。批量回归运行 `scripts/browser_v2.cjs`、`scripts/browser_review_fixes.cjs` 与 `scripts/browser_generic_parameters.cjs`（后者独立覆盖通用共享参数链与可选 `math` 扩展；必要时加 `scripts/browser_ui_refactor.cjs`），`NCTOOL_BINARY` 指向固定程序；Playwright/Chromium 方法见[贡献指南](CONTRIBUTING.md)。工具无法连接时记录限制，继续隔离自动回归；脚本通过不代表手工或全部平台验收通过。

## Context7

使用已安装的 Context7 插件：`resolve-library-id` → `query-docs`。先读 Cargo/npm 锁文件，查询包含实际版本与一个具体问题；无精确版本时查官方对应版本文档/源码补证，不据此升级依赖。

例如解析 `Vitest`，选择返回的 `/vitest-dev/vitest/v5.0.3`，查询 `vitest run` 的单次运行行为。仍须核对返回链接：版本化查询可能返回 `main` 的片段。本仓库 `npm run --prefix ui test` 已显式使用 `vitest run`。

安装、工具加载与查询成功是不同状态。插件工具可用时无需重复配置 MCP。查询只包含公共库信息和最小问题，不发送凭据、运行资产或非公开源码。说明见 [Context7 官方仓库](https://github.com/upstash/context7)。

## 依赖审计

验收报告需与历史证据分开时，`scripts/browser_*.cjs` 支持 `NCTOOL_REPORT` 指定各自输出目录；报告中的 WebUI 路径按实际输出位置记录。固定二进制、隔离目录及完整示例见[贡献指南](CONTRIBUTING.md)。

默认检查根 `Cargo.lock` 与当前 `ui/package-lock.json`：

```bash
cargo install cargo-audit --version 0.22.2 --locked
python scripts/audit_dependencies.py
```

不安装项目依赖、不修改锁文件、不自动修复。报告、RustSec 数据库、npm 缓存默认放在忽略目录 `target/dependency-audit/`。需要网络：npm 向配置的 registry 发送依赖信息，Rust 审计访问 RustSec 与 crates.io。

| 结果 | 门槛 |
| --- | --- |
| Rust | 已知漏洞失败；unmaintained 等 warning 保留供人工核对 |
| npm | high/critical 失败；low/moderate 保留且不阻断 |
| 工具/网络/报告错误 | 未完成并失败，不当作零漏洞 |

退出码 0 = 完成且未命中；1 = 有命中项；2 = 工具/网络/报告错误或锁文件变化。`summary.json` 保存时间、命令、状态及锁文件 SHA-256，原始 JSON/stderr 独立保存。再次运行同一输出目录会覆盖报告，需留证时使用不同 `--output`。

CI 新增 `dependency-audit` 作业，推送/PR 与发布复用验证均执行，失败仍上传报告。Dependabot 覆盖当前 `/ui`、Cargo 和 Actions。旧 Tauri 桌面源码、锁文件及专属审计配置已移除；审计入口仅检查当前产品锁文件。新作业是否通过须查实际 Actions，本地配置不等于远端运行。

处理发现先区分直接/间接、运行/开发依赖，核对公告、可达入口、受影响版本、修复版与兼容性。不要执行 `npm audit fix --force`、批量更新或无依据隐藏告警；审计与依赖更新分别授权。依据 [RustSec](https://rustsec.org/) 与 [npm audit](https://docs.npmjs.com/cli/v11/commands/npm-audit/)，审计通过不等同于没有安全缺陷。
