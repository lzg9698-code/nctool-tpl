---
name: nctool-verify
description: 为 NCtool 改动选择并执行验证，运行隔离 CLI/WebUI 验收、依赖审计及诊断 GitHub CI；在验证修复、验收交互、检查依赖或排查失败测试时使用。
---

# NCtool 验证

先读仓库 [AGENTS.md](../../../AGENTS.md) 和 [贡献指南](../../../docs/CONTRIBUTING.md) 的相关段落。按改动选择验证，不用历史测试数量代替结果；记录源码 SHA、是否有未提交修改、平台与命令。

## 选择检查

| 改动 | 相关入口 |
| --- | --- |
| Rust/插件/宿主 | 对应包测试及正常、失败、边界；跨层修改增加 workspace/协议/CLI 回归 |
| WebUI | `npm run --prefix ui test`，生成资源后编译宿主，再检查相关浏览器场景 |
| 架构/生成资源/打包 | `python scripts/check_v2.py`，确认纯模板依赖树、`math` 唯一可选 feature 与包内容 |
| Python 验证工具 | `python -m unittest discover -s scripts/tests -p 'test_*.py'` |
| 依赖或安全审计 | `python scripts/audit_dependencies.py`，核对锁文件未改变及原始报告 |
| 纯文档 | 链接、锚点、接口与实际采用示例；不强制无关全量编译 |
| 发布准备 | 完整 CI、MSRV、三组浏览器验收与实际远端平台结果 |

完整 Rust 命令带 `--workspace`、`--locked`；不要省 workspace 导致只检查根 crate。前端测试脚本已使用 `vitest run`。第三方工具选项不确定时从锁文件定位版本，再用 Context7 查具体问题，并核对官方对应版本来源。

## 隔离验收

- 使用独立 target（例如 `target/codex-math`）构建 `nctool-cli --features math --locked`；默认组合另建独立目录。固定 `NCTOOL_BINARY`，不覆盖运行中的二进制。
- CLI 示例引用 `examples/v2`，显式指定临时 `--home`/`--workspace`。不得使用用户运行资产。
- 浏览器工具先读取工具文档，在本轮启动的 `ui --port 0` 地址检查控件。`--profile` 仅支持 `template`；数学扩展需以 `--features math` 构建并在配置 `enabled` 与请求 `extensions` 中显式启用/选择。按改动选择草稿、冲突、失败、取消、导出、历史预检查及迁移场景。
- 自动回归使用现有 `scripts/browser_v2.cjs`、`scripts/browser_review_fixes.cjs` 与 `scripts/browser_generic_parameters.cjs`（后者覆盖通用共享参数链与可选 `math` 扩展）；Playwright/Chromium 放在独立依赖目录，以 `NCTOOL_PLAYWRIGHT`/`NCTOOL_CHROMIUM` 配置，不添加到产品依赖。
- 只停止本轮进程/关闭本轮标签页；浏览器受限时记录原因，继续可用的隔离验证，不借用现有用户工作台。

## CI 与审计证据

GitHub 工具读取目标提交的检查、Actions 和失败日志，远端结果与本地工作树分别记录。连接器结果为空或仅列出 PR 运行时，以只读 `gh run list/view` 补充；不能直接判通过。远端运行失败先定位失败命令，修复授权范围内的问题，再做相关本地复现。

审计报告默认 `target/dependency-audit/summary.json`；退出码 1 是命中门槛，2 是检查未完成/锁文件变化。低等级或维护状态告警仍需列出。处理方法见 [工具指南](../../../docs/CODEX_DEVELOPMENT.md)，不自动更新依赖或忽略公告。

交付实际已通过、失败、未执行项和报告/运行链接。全部通过后不为凑检查数量重复运行。测试无法证明生产使用安全，也不代表其他未运行平台的状态。
