# NC 最终处理链收敛与验证

日期：2026-10-13。平台：Windows 11。基准提交：`a7a507d7529ad8a2f722ca00836bc100b59c0517`；验证针对包含既有未提交/未跟踪内容的本地工作树。Rust 1.98.0 / Cargo 1.98.0。没有提交、推送或发布。

## 已完成

- `plugins/nc/src/lib.rs` 提取插件内部共享的选项读取与最终输出处理函数。普通 NC 生成和组合文本最终处理共用格式限制、后处理、输出非有限数检查、行号游标和告警；最终处理前后检查取消。
- `nc.output.finalize` 不再将已生成文本作为 `program` 参数，用 `{{ program }}` 重新走模板检查与渲染。它直接验证机床内容并消费文本，保留 `__inline__` 头部标识、原输出名、正文与诊断契约。无关模板资产损坏不再阻断已生成文本的最终处理。
- `nc.output.finalize` 不再将已生成文本作为 `program` 参数，用 `{{ program }}` 重新走模板检查与渲染。它直接验证机床内容并消费文本，缺失字段、错误类型和 `null` 返回结构化错误而不是进程 panic；保留 `__inline__` 头部标识、原输出名、正文与诊断契约。无关模板资产损坏不再阻断已生成文本的最终处理。
- 修复新生成的 `nc.output.finalize` 记录重放参数：保存其自身的 `name/text/machine/options`，避免宿主用原动作重执行时收到 `nc.generate` 的输入结构。原历史内容不改写，旧错误格式记录的迁移未在本轮实施。
- 增加四项宿主集成回归：普通生成与最终处理的正文/行号/诊断一致性；无关模板损坏隔离和文本中 Jinja 标记保持字面含义；机床/选项/文件名/非有限输出、类型缺陷与取消拒绝；真实后台记录重执行及原记录不变。
- 同步架构说明与未发布变更记录。现有工作区改动保留；本轮没有前端编辑、生成资源重建、依赖或锁文件变化，也未读取或修改用户运行资产。

## 实际验证

| 检查 | 本轮结果 |
| --- | --- |
| `cargo test --workspace --all-targets --all-features --locked --target-dir target/simplification-nc` | 470项通过，包括4项新增回归、受管组合和专用车削的输出/失败原子性回归 |
| `cargo test --workspace --locked --target-dir target/simplification-template configuration_` | 默认组合3项配置回归通过 |
| `python scripts/check_v2.py` | 纯模板依赖树、领域边界、包内容与生成资源检查通过 |
| `cargo test --workspace --all-features --doc --locked --target-dir target/simplification-nc` | 4项通过 |
| `cargo clippy --workspace --all-targets --all-features --locked --target-dir target/simplification-nc -- -D warnings` | 通过 |
| `cargo fmt --all -- --check` | 通过 |
| `git diff --check` | 通过；Git提示已有docs/README.md的CRLF转换 |

初次定向测试命令为 `cargo test --workspace --all-targets --all-features --locked --target-dir target/simplification-nc nc_finalize`，结果1项通过、3项失败。失败来自本轮测试夹具缺少机床必填 `config` 和未知选项错误码预期写成 `invalid_input`（实际为 `invalid_json`）；修正夹具与显式类型校验后，最终全量回归包含四项并全部通过。中途两次失败日志（`nc-finalize-workspace-tests2.log` 的类型缺陷输入被 Schema 拦为 `invalid_input`、`nc-finalize-tests.log`）保留，不改写为通过。最终日志为 `nc-finalize-workspace-tests3.log`、`nc-finalize-clippy2.log`、`nc-finalize-doc-tests2.log`、`nc-finalize-template-tests.log` 与 `nc-finalize-architecture.log`。

本地日志位于忽略目录 `target/simplification/`：`nc-finalize-tests.log`（初次失败）、`nc-finalize-workspace-tests.log`、`nc-finalize-doc-tests.log`、`nc-finalize-clippy.log`。本轮源码SHA-256和计数保存在 `nc-finalize-evidence.json`。日志与本地证据不保证随仓库分发。

## 未执行与遗留

- 未运行浏览器、Python、前端、默认template组合、MSRV 1.89、Linux/macOS、远端CI或依赖审计；本轮没有修改对应入口或依赖，也不是发布准备。没有用历史结果代替本轮验证。
- 旧workbench迁移/退役、资源按需读取和CSS清理仍待后续收敛；本轮不扩大到这些项。
- 轴类实际输入、外部模拟、真机测试和工艺审核仍按独立产品的验收边界推进；Rust软件回归不能作为机床加工验收。

前一轮记录见[项目精简第一轮](2026-10-13-simplification-iteration.md)，实施依据见[精简审查建议](../../PROJECT_SIMPLIFICATION_REVIEW_2026-10-13.md)。
