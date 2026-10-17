# 2026-10-15 Rust 平台清理与最终本机验证

本次按用户授权清理旧资产和所有已识别剩余问题，以当前 Rust 通用模板平台功能为交付范围。Windows 11，Node 24.15.0，Python 3.13，稳定 Rust；工作树基于 `e8fafd5` 且包含此前未提交精简与 UI 改动。未提交、推送或发布。

## 实施

- 删除依赖仓库外轴类产品的 `scripts/dev/workbench_migrate.py` 和 `scripts/tests/test_workbench_migration.py`；保留平台内置配置预览/显式迁移、退役标记拒绝和历史只读边界。
- 删除五份无用专题：整套程序生成研究、下一代辅助编程研究、轴类首版交付、一次性审查提示词、专用车削方案。现行文档不再依赖外部产品或私有研究目录；有历史反向引用的旧路径缩为说明入口，已有恢复包与归档原字节保持。
- 清除旧 `ui/src/workbench` 类型检查排除和构建指纹跳过分支。将前端 lint 与第三组浏览器验收纳入 CI，更新 Issue/PR 模板为当前 Rust 模板、参数、插件和任务边界。
- 设置页隐藏 `retired` 能力；共享参数默认启用、禁用优先、跟随 template 及显式启用行为与后端一致。新增六项交互测试。
- 删除共享参数视图无效的动态导入写法，消除静态/动态重复导入构建告警。
- 移除资产数值模块无活动消费者的 YAML 词法分支和仅 YAML 测试，删除 `serde_yaml` 开发依赖及 `unsafe-libyaml`。保留 JSON 整数精度、次正规数、下溢和 Unicode 定位检查，新增 JSON 边界测试。
- 将被撤回的传递依赖 `yoke-derive` 从 0.8.3 定向更新到 0.8.4；没有批量升级依赖。

## 最终验证

以下全部针对本轮最终源码实际执行：

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 通过 |
| `cargo test --workspace --all-targets --all-features --locked` | 248 项通过 |
| `cargo test --workspace --all-features --doc --locked` | 2 项通过 |
| `cargo test --workspace --all-targets --locked` | 默认组合 248 项通过 |
| `cargo +1.89 check --workspace --all-features --locked` | 最低支持版本通过；本机工具链名称为 1.89 |
| CLI 默认无 math 与 `--features math` 独立构建 | 通过 |
| `npm run --prefix ui lint` | 通过 |
| `npm run --prefix ui test` | 12 文件、62 项通过 |
| `node scripts/build_ui.mjs` 与 `--check` | 正式资源构建及嵌入一致性通过 |
| `python scripts/check_v2.py` | 通过 |
| `python -B -m unittest discover -s scripts/tests -p 'test_*.py'` | 24 项通过，无旧迁移导入失败 |
| 三组 Chromium 验收 | 全部通过 |
| 现行文档相对文件链接检查 | 168 项，无失效目标 |
| `git diff --check` | 通过，只有既有换行转换提示 |

Rust 默认和 math 使用独立 `target/final-platform-20261015/default`、`math`，MSRV 使用 `msrv`。浏览器固定 math 二进制，自建临时 home/workspace 和随机 loopback 端口，未访问用户运行资产。

三组报告：

- `target/final-platform-20261015/browser-main/report.json`：模板、受管参数、显式 math、插件、历史和集合包。
- `target/final-platform-20261015/browser-boundaries/report.json`：无效输入、精度、草稿恢复、失败/取消/过期交付及退役边界。
- `target/final-platform-20261015/browser-ui/report.json`：主题与草稿刷新恢复、编辑器实例保持、菜单/对话框焦点、明暗对比度、1280×720/1440×900/1920×1080/375×812 布局。

## 依赖审计

初次自动下载 RustSec 超时，npm 请求也遇到 TLS 中断。随后使用 Git 获取同一官方 RustSec 数据库，为统一审计入口预置其副本，再实际运行 `python scripts/audit_dependencies.py --output target/final-platform-20261015/audit-complete`。

最终 `summary.json`：Rust/npm 均退出码 0，审计期间锁文件未变化；Rust 原始报告为 0 漏洞、空 warnings，npm 0 漏洞。数据库提交 `edb58f5974b83eb8302332821cb6d389777aa351`。未设置忽略公告、跳过漏洞或放宽版本限制。原网络失败报告留在 `dependency-audit/`，最终报告在 `audit-complete/`。

## 范围限制

这些结果证明当前 Windows 本机源码的验证状态；未执行本轮 Linux/macOS 远端 CI，未创建发行资产、标签或 Release。新增 CI 检查步骤尚需相应提交的远端结果，不能把本机通过当作跨平台发布完成。已有归档中的旧测试数量、失效历史路径和审计状态仅描述当时事实，不改写。
