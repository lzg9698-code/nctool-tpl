# NCtool 阶段收尾与本地提交（2026-10-08）

## 范围与恢复证据

平台为 Windows，分支为 `master`，基准 HEAD 为 `f0fda2d4e888e70fa5b0726ec77fd7088b6e95d3`。本次收尾覆盖基准之上的源码、测试、示例、生成资源、文档归档、开发规则和工作流改动；不新增业务功能、不升级依赖、不推送、不创建 PR、标签或发布。

修改前用 `scripts/source_baseline.py` 建立并验证 `.nctool-backups/closeout-20261008/`，包含 464 个源码文件。恢复包 SHA-256 为 `728b5e157f91ba079f2c34d45e50b3c03c20c907f288c9328f4e45c15cb9c11e`。其 manifest、原始 status 与 staged/unstaged patch 区分既有改动和本轮修复；原始暂存区为空。

本轮独立报告目录为 `target/closeout-20261008/`，日志、退出码、UTC 执行时间及平台记录于各项 `.log`、`.result.json`。这些忽略目录内的本地证据不随提交携带；本文件记录实际结论，不把历史结果当成本次通过依据。

实际验证工作树的非文档文件指纹为 `ab930983b348bd05fa8b10b8d4dd4afa068b54badba5658856bc16b1225840a0`，245 个文件；约定是 Git 跟踪及非忽略未跟踪的现存文件，排除 Markdown 和 `docs/`，按相对路径排序，对每项 `path + NUL + 文件 SHA-256 + 换行` 再计算 SHA-256。完整路径清单在 `tested-source-manifest.json`。提交前按 `stage-candidates.json` 明确暂存并检查差异；提交 SHA 由提交完成后的交付说明记录，避免文件自引用。

原有 `cli/ui/source.sha256` 内容 `efeabefd3424873081e1995e19a6f9cffe45e3cd109f5222e112e0571241be47` 原字节保留，生成资源一致性通过，没有为清理状态重建。根目录 `parameters/`、`templates/` 加入根忽略规则，170 份 JSON 原字节未变，不参与提交。166 份与可导入包对应相同，另有本地 test 模板；三份不同模板的旧源码与新包装内 `metadata.nc.legacy_source` 一致，核对兼容而未同步覆盖。受管对照使用 `managed-assets/` 示例副本，未让脚本改写提交候选示例。

## 审查发现与处理

按参数解析 → NC 原始输入/计算 → 版本化服务 → CLI/HTTP → 多工序与文件交付审查，结合现有回归检查精确整数、非有限值、对象/null 领域边界、指纹冲突、失败无部分交付、不可变历史及环境漂移。审查专用车削页面、ap2 推荐、草稿恢复与变化后确认、模板切换、过期导出和关闭插件后的能力退化；核对默认模板组合、NC 依赖隔离、旧 GUI 删除及 CI 配置。

| 发现 | 修复与回归证据 |
| --- | --- |
| 共享资产 remove 接受 `expected: null` 后 unwrap，CLI 可 panic 退出 101 | remove Schema 只允许字符串，执行层防御校验；测试非法类型不删资产、真实过期指纹冲突及当前指纹成功删除。save 新建 null 基线保持兼容 |
| `nc.managed.render` 接受不完整冻结/解析结果后 unwrap | render/resolve 复用冻结校验，检查工序、contexts、定义与现有 ParameterSet；测试缺字段、错误结构、非法 NC 上下文及随后正常渲染 |
| 专用车削历史快照或轨迹预设畸形结构触发可变 JSON 索引 panic | 校验 assets 版本/records/templates，复用底层 NC freeze 校验；预设 asset/parameters 必须为对象。测试畸形输入拒绝及后续正常生成，保留冻结历史而不重读当前资产 |
| CLI 禁用后无法重新启用三个已有内置插件 | enable 内置清单补齐 `nc-managed`、`sync-turning`、`undercut`；真实 CLI 禁用/启用、配置及重启后能力目录回归 |

复现日志保留为 `remove-null-before.json`、`render-invalid-before.json`、`snapshot-invalid-before.json`、`enable-builtin-before.json`。修复后固定二进制的 CLI/HTTP 边界检查通过：非法请求返回结构化错误，原资产保留，后续生成成功，报告 `input-boundaries.json`。新增 Rust 回归包含实际冲突和恢复行为。

本轮工具修正仅增加报告位置配置，保持默认目录兼容：八组浏览器支持 `NCTOOL_REPORT`，受管浏览器支持 `NCTOOL_COMPARISON_REPORT`，专用 Python 对照支持 `--report`。原有报告未覆盖；最后一次 CLI 修复前的通过证据与二进制保存在 `before-cli-enable/`，最终业务/浏览器均重新执行。

未发现尚未解决的本地提交阻塞缺陷。较大的前端职责拆分沿用[路线图](../../ROADMAP.md)，本轮未扩大功能范围。CLI/WebUI 2.2、组件 2.0、独立 v1 服务/协议策略保持不变。

## 实际验证结果

默认和 NC 使用独立 target，验收固定二进制如下；均使用本轮临时 home/workspace，浏览器服务绑定 loopback 并使用 `ui --port 0`，只清理本轮创建的服务。

| 二进制 | SHA-256 |
| --- | --- |
| `target/closeout-20261008/nctool-nc.exe` | `71ce20a76161604a1836d5645e44f1945d75ff10514652c11fab3b5ea9bab124` |
| `target/closeout-20261008/nctool-template.exe` | `edd88174625468aa4d50fd97a25b86314169718563dd63cef1c61942b257f850` |

| 检查 | Windows 结果 / 本轮报告 |
| --- | --- |
| Workspace 全 targets/features 测试 | 463 项通过；`rust-tests-commit-candidate.log`，另有 benchmark smoke 成功 |
| 文档测试 | 4 项通过；`doc-tests-final.log` |
| clippy / fmt | 通过；`clippy-commit-candidate.log`、提交前格式检查 |
| 默认组合检查/构建、NC 构建 | 通过；`template-check-final.log`、`template-build-commit-candidate.log`、`nc-build-commit-candidate.log`；默认 CLI 参数测试另有 2 项通过 |
| Rust 1.89 MSRV | workspace all-features check 通过；`msrv-commit-candidate.log` |
| 前端 / Python 工具 | 32 项 / 14 项通过；`ui-tests.log`、`tools-final.log` |
| 架构 / 生成资源 | `check_v2.py`、`build_ui.mjs --check` 通过，11 项资源一致 |
| 受管参数 CLI/HTTP 对照 | 142 项通过；`managed/report.json` |
| 专用车削 Python/Rust 对照 | 115 项通过，88 个冻结源文件哈希核对；`specialized/report.json` |
| browser_v2 / browser_review_fixes | 原有两组端到端回归通过；各自报告目录 |
| browser_managed_parameters | 126 项通过 |
| browser_specialized_turning | 26 场景通过 |
| browser_specialized_templates / browser_trajectory_presets | 各 8 场景通过 |
| browser_turning_ui / browser_ui | 9 场景 / 通用工作台布局交互验收通过 |
| 当前指南和示例 | 9 组实际 CLI 示例通过；本地链接/锚点检查通过；历史融合四份 JSON/patch 与基准 Git 原字节一致 |
| 依赖审计 | Rust/npm 无门槛命中；保留 yoke-derive 0.8.3 撤回 warning，锁文件未变 |

不同验收集存在交集，数量不累加为独立功能总数。Windows incremental 缓存硬链接不可用时 Rust 回退复制，检查退出码成功；没有将工具提示改写为源码失败或省略必要检查。

### 执行命令

以下路径均相对仓库根；日志中的命令记录保留实际绝对路径。依赖解析命令使用 `--locked`。

```powershell
cargo test --workspace --all-targets --all-features --locked --target-dir target/closeout-nc
cargo test --workspace --doc --all-features --locked --target-dir target/closeout-nc
cargo clippy --workspace --all-targets --all-features --locked --target-dir target/closeout-nc -- -D warnings
cargo fmt --all -- --check
cargo check --workspace --locked --target-dir target/closeout-template
cargo build -p nctool-cli --locked --target-dir target/closeout-template
cargo build -p nctool-cli --features nc-bundle --locked --target-dir target/closeout-nc
cargo +1.89 check --workspace --all-features --locked --target-dir target/closeout-msrv
npm run --prefix ui test
python -m unittest discover -s scripts/tests
python scripts/check_v2.py
node scripts/build_ui.mjs --check
python scripts/verify_managed_parameters.py --binary target/closeout-20261008/nctool-nc.exe --assets target/closeout-20261008/managed-assets --report target/closeout-20261008/managed
python scripts/verify_specialized_turning.py --binary target/closeout-20261008/nctool-nc.exe --report target/closeout-20261008/specialized
```

八组浏览器逐一执行 `node scripts/<上表脚本名>.cjs`，设置 `NCTOOL_BINARY`、`NCTOOL_TEMPLATE_BINARY` 为上述固定路径，`NCTOOL_REPORT` 为各自本轮目录；受管浏览器读取本轮 `managed/report.json`。Playwright 1.62.1 独立安装于忽略工具目录，使用已安装 Chromium 1208。初次受限环境 Chromium/loopback 失败的日志保留，允许联网与启动浏览器后实际完成验收。

### 审计网络恢复与原始证据

先实际执行 `python scripts/audit_dependencies.py --output target/closeout-20261008/audit`，受限网络失败；联网重试 `--output target/closeout-20261008/audit-online` 时 npm 成功，但 cargo 内置数据库 fetch 超时 300 秒。两次结果均保留为未完成，没有按零漏洞处理。

随后使用 Git CLI 从 `https://github.com/RustSec/advisory-db.git` fresh shallow clone 到 `audit-cli/advisory-db`，审计前用 `git ls-remote ... HEAD` 核对 local/remote 同为 `b8a1a33e246a0a9a3b5f377248c41a503defec74`。本轮 `audit_fresh.py` 复用原审计脚本 main、报告解析、锁文件检查及门槛，只为已经刚取回且核对的数据库给 cargo 增加 `--no-fetch`。实际执行：

```powershell
cargo audit --file Cargo.lock --db target/closeout-20261008/audit-cli/advisory-db --json --no-fetch
npm audit --prefix ui --audit-level=high --json --cache target/closeout-20261008/audit-cli/npm-cache
```

最终 `audit-cli/summary.json` 中两项 status 均为 ok、退出码 0，原始 `cargo.json`、`ui.json`、stderr 及数据库 HEAD 元数据保留。没有使用过期库跳过检查、添加忽略公告或修改版本；yoke-derive 0.8.3 撤回属于保留维护告警。

| 锁文件 | 审计前后不变的 SHA-256 |
| --- | --- |
| Cargo.lock | `dbecc581dec5f864223e428575fb73aa06dc0e66829f29a12683410576f81c4b` |
| ui/package-lock.json | `1e05c3c06bfc4aeecf0b84ef8d83c50856b3bdcd26f3e4cf1ab348596c2b81b9` |

## 提交与剩余事项

完整暂存差异的严格空白检查发现两处提示：`cli/ui/assets/index-CzIKHKMR.js` 与 `ui/dist/assets/index-CzIKHKMR.js` 第 24 行均为语法指南“控制空白与换行”的模板字符串 `A ` 行末空格，来自已有 `ui/src/lib/syntaxGuide.ts`，属于实际示例输入。其余文件严格检查通过；这两个生成文件单独关闭 blank-at-eol 检查后通过，生成资源一致性仍通过。未手改生成代码或删除示例的有效空格。

本次本地提交标题为 `feat: complete managed parameters and specialized turning workbench`。提交候选包含旧 GUI 删除、历史资料对应归档、新实现与示例、正式脚本生成的 UI 资源、开发规则与 CI；排除运行资产、日志、临时依赖和恢复包。暂存后核对删除与归档对应、生成资源、差异空白及没有执行期间的无关文件混入。

本次必需本地检查已完成，无未解决的提交阻塞项。尚未执行目标提交的远端 Linux/Windows/macOS CI，也未执行真实控制器或机床工艺验收；本地软件通过不替代它们。未执行推送、PR、标签或发布。依赖维护告警和前端进一步拆分作为后续维护事项，需独立评估。
