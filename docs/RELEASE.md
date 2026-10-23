# 发布说明

适用于当前 NCtool 2.x CLI/WebUI。以仓库[发布工作流](../.github/workflows/release.yml)和 [CI 工作流](../.github/workflows/ci.yml)为事实依据；本页说明流程，不表示已经执行发布。

## 版本与发行内容

CLI/WebUI 当前清单版本为 2.3.0（未发布），其他 workspace 组件当前为 2.0.0；插件的声明版本、协议版本和服务版本分别维护。变更记录放在根 CHANGELOG 的未发布节，正式发布日期只在实际发布后登记。

当前发布工作流由 `v2.*` 标签推送触发，构建带 `math` feature 的 CLI；默认配置启用 `template` + `parameters`，数学扩展需在配置 `enabled` 中启用并在请求 `extensions` 中显式选择。二进制包含 WebUI，运行不需要 Node.js；旧 GUI、旧业务模板及内置机床方案不在发行范围。

**现有工作流没有 crates.io 上传步骤。** 不沿用旧版三个 crate 的顺序上传或旧候选标签流程；crate 发布需要另行明确范围和流程。

## 发布前检查

- [ ] 确认待发布提交和工作树状态；本地源码快照与正式提交分别记录，保留用户已有改动。
- [ ] 核对组件版本、CHANGELOG 未发布内容、快速开始、动作示例与兼容性说明。
- [ ] 按[贡献指南](CONTRIBUTING.md)完成相关本机检查和浏览器验收，记录平台及实际结果。
- [ ] 明确旧 CLI/API/资产不自动迁移，以及旧加工边界：配置不自动迁移、旧模板/冻结引用拒绝降级、旧历史只读、混合加工资产包整体拒绝。
- [ ] 确认实际发布标签属于 `v2.*` 并对应准备好的提交，再按仓库发布操作推送；本文不自动执行标签或发布。

## 预览版标签

预览版使用带后缀的标签，例如 `v2.3.0-preview.1`。工作流对含 `-` 的标签设置 `prerelease: true` 并把 `make_latest` 设为 `false`，因此预览版不会覆盖最新稳定版；资产、校验文件和验证门槛与正式版相同。预览版可以包含破坏性变更，必须在 Release notes 与 CHANGELOG 中说明旧版本是否仍可回退使用。

## 工作流与验证门槛

标签触发后，`validation` 复用完整 CI：三平台前端安装/测试/构建，Rustfmt、Clippy、workspace 测试和文档测试、架构/包内容检查、源码快照测试、纯模板构建；Linux 执行三组 Chromium 验收，另有 Rust 1.89 MSRV 检查。

`binaries` 依赖 validation 成功，再按明确 target 构建完整 CLI；`publish` 依赖二进制矩阵完成，下载产物并创建 GitHub Release。任一验证/构建失败不会继续后续发布，不跳过门禁重发。当前工作流使用自动生成的 Release notes，发布后检查其与 CHANGELOG 和兼容性说明一致。

完整 CI 还包含 `dependency-audit`：根 Cargo 与当前 `ui` 锁文件审计，Rust 漏洞及 npm high/critical 阻断验证，工具或网络错误也失败。报告以 Actions artifact 保存；具体规则见 [Codex 开发指南](CODEX_DEVELOPMENT.md)。新增门槛的实际运行状态须查待发布提交的 Actions，不以配置存在代替通过。

| 平台 | Rust target | 二进制资产 |
| --- | --- | --- |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `nctool-linux-x86_64` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `nctool-windows-x86_64.exe` |
| macOS ARM64 | `aarch64-apple-darwin` | `nctool-macos-arm64` |

每个二进制都有同名附加 `.sha256` 的文件，内容为摘要和文件名。

## 发布后签收

- [ ] 记录标签、提交、Actions 运行链接、各平台结论与 Release 链接，确认全部预期资产存在。
- [ ] 下载相应平台资产，核对 SHA-256，再执行 `nctool --version`、`actions` 与代表性渲染。
- [ ] 检查默认 `template` + `parameters` 组合与完整二进制的 `math` 扩展，WebUI 可启动、输出可实际下载。
- [ ] 在项目状态与 CHANGELOG 登记实际结果和日期，未执行的平台/设备保留待验状态。

PowerShell 校验命令为 `Get-FileHash -Algorithm SHA256 <二进制路径>`；Linux 可使用 `sha256sum -c <校验文件>`，macOS 可使用 `shasum -a 256 -c <校验文件>`。在二进制与校验文件所在目录执行并核对文件名。

SHA-256 用于完整性检查，不等于发布者身份认证或工艺签收。旧版发布策略、1.0 候选与 crates.io 演练记录见[历史发布说明](archive/legacy/RELEASE.md)。
