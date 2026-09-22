# 本轮继续开发概览

> 更新：2026-09-22。本文是**当前快照摘要**，数字均为实测值。
> 权威现状见 `docs/PROJECT_STATUS.md`，规划见 `docs/ROADMAP.md`。

## 当前位置

- 阶段 A–F **全部收口**（ROADMAP 执行跟踪 70/72，97%）。
- 三个 crate 已发版 crates.io：`nctool-tpl` 0.4.0 / `nctool-core` 0.3.0 / `nctool-cli` 0.3.0，
  并挂三平台 GitHub Release 二进制。
- CLI 10 个子命令可用（除 `part` 占位）；Web UI 完整交互闭环。
- 模板库：**7 个内置模板 + 25 个文件模板**（NCTool_V3 资产已整合）。

## 质量基线（2026-09-22 实测）

| 门 | 结果 |
|---|---|
| workspace 测试（`--all-targets`） | **579 通过 / 0 失败**（另 2 项 `#[ignore]`） |
| Doc-test | 1 通过（`src/lib.rs`）+ 1 `#[ignore]` |
| 覆盖率（生产口径） | **90.34%**（4553/5040），门禁 **≥ 89%** |
| 覆盖率（llvm-cov 原始口径） | 94.04%（仅参考，不作门禁） |
| `cargo fmt --all --check` | 干净 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零告警 |
| MSRV | 1.85（三个 Cargo.toml 与 CI `msrv` job 一致） |
| CI | 三平台矩阵 + 覆盖率门 + 前后端对拍门，均为阻断项 |

## 剩余工作

- **第四轮审查批次 E 的 UI 两项**：P2-25 Bool 参数恒提交 `false`、P2-26 无
  `AbortController`（均需浏览器验证）。批次 D 的服务层两项（P1-10 CLI 读文件上限、
  P1-11 `PipelineError::source()` 漏 `Derive`）已于 2026-09-22 收口。
- **覆盖率洼地**：`cli/src/commands/ui.rs` 20.00%（全项目最低）、
  `cli/src/commands/inspect.rs` 80.79%、`core/src/variables.rs` 83.70%。
- **发布流程**：给三个 crate 加 `include`/`exclude` 收窄发布包；下次发版先推 tag 交 CI 发布。
- **Backlog**：`nctool part generate`、内置模板补外圆车削与攻丝、参数集命名预设、`nctool lint`。
- **长期外部依赖**：真实工艺评审与机床空运行（R1，Q2=否）—— 代码无法解决。
