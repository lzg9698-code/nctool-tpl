# 独立参数与完整工艺迁移实际验证

2026-10-07，Windows x86_64。验证对象为 HEAD `f0fda2d4e888e70fa5b0726ec77fd7088b6e95d3` 上的未提交工作树，不是 Git 提交或发行版。CLI/WebUI 仍为 2.2、核心组件为 2.0。按 [nctool-verify](../../../.agents/skills/nctool-verify/SKILL.md) 执行，资产和当前接口见 [参数指南](../../PARAMETER_MANAGEMENT.md)。

独立参数底座、NC 资产扩展、Rust 工艺处理器、三个受管生成服务和界面接入已实现。全部 28 份源模板提供为 [工作区资产包](../../../examples/v3-managed/workspace.json)：26 份业务模板通过实际生成，2 份公共片段通过全部依赖模板验收。逐模板计算映射和可携带验证数据分别见 [mapping.json](../../../examples/v3-managed/mapping.json) 和 [validation.json](../../../examples/v3-managed/validation.json)。

## 构建与来源

- 固定验收二进制：本地 `target/parameters-validation/nctool-complete2.exe`，SHA-256 `17fda7b09041cc9357daa667ccdaa54f0e45f3ba4d41992375c909a55b4038e9`。CLI、HTTP、全部浏览器验收使用这一份文件。
- 工具：Rust 1.98.0、Node.js 24.15.0、Python 3.11.6；NC 与默认组合分别使用 `target/parameters-nc`、`target/parameters-template`。
- V3 对照仅使用 `target/parameters-validation/frozen-v3` 中冻结的 core/configs/templates。67 个源文件和冻结文件逐项核对 SHA-256，全部保持一致；源目录未写入。
- 开始时的 221 文件恢复归档 `.nctool-backups/parameters-20261007` 完整性检查通过，保留原先未提交内容。前端指纹的原始内容保存在该归档中，本轮变更来自正常 UI 构建。
- 新增 Cargo 内部 workspace 包，没有升级外部 Rust/npm 依赖。资产包、模板、源文件和重写记录具有独立指纹；未把历史输出或零件尺寸作为共享默认。

## 本次执行结果

| 检查 | 结果 |
| --- | --- |
| Rust workspace 所有 target/features 测试 | 446 通过 |
| Rust 文档测试 | 4 通过 |
| 前端单元测试 | 20 通过 |
| Python 验证工具测试 | 14 通过 |
| Python/Rust 计算、程序、CLI/HTTP 原始输入验收 | 142 项通过，0 失败 |
| 受管模板 WebUI 验收 | 126 项通过，0 页面异常 |
| 两组既有浏览器回归 | browser_v2 / browser_review_fixes 全部通过 |
| 默认 workspace 检查/构建、NC workspace 构建 | 通过 |
| fmt、clippy -D warnings、架构/生成资源检查 | 通过 |
| 依赖审计 | cargo/npm 门槛通过，锁文件不变 |

CLI/HTTP 的 142 项由 100 组计算对照、19 份普通/主程序模板、左右各一组批量同步、20 组非法输入一致拒绝，以及一组后续工序失败测试组成。100 组计算包括键槽三个阶段和五种形式、分层边界与不开倒角，左右同步车的整数/近整数/无切削分支，34 个 ES/FS 标准两侧，以及卡簧槽多位置和受限数值表达式。所有计算样本均直接提交原始业务输入，不注入中间结果。

对照逐项检查中间上下文、分支和同步推进：离散结果严格相等，连续结果使用绝对 1e-11 / 相对 2e-13 的算法误差界限，并核对最终输出精度。程序只允许预先声明的 LF、每行首尾空白和末尾换行适配；数值、指令、通道、顺序和同步编号不被忽略。

WebUI 经实际控件提交全部原始输入；原生模板不显示要求填写的派生字段。另覆盖当前片段主程序绑定、可视化多工序、组合中途失败无导出、资产保存冲突、空/非法草稿阻止生成与恢复、加工预置选择/人工转速确认及资源变更后禁止导出。Rust 集成测试覆盖冻结重执行、默认来源保持、Schema/单位交集、派生伪造拒绝、刀具事实、加工换算、限速拒绝和多文件原子交付。

## 逐模板签收

| 稳定模板 ID | 工艺 | 程序样本 | 计算对照 | CLI / HTTP / WebUI |
| --- | --- | --- | --- | --- |
| `v3-circlip-groove-template` | circlip | 3 组 | 适用并通过 | 通过 |
| `v3-index-g420-1-0.mpf-a` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-1-0.mpf-b` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-2-0.mpf-a` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-2-0.mpf-b` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-3-0.mpf-a` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-3-0.mpf-b` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-dg-cal-ir9.spf` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-uz-dj-x` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-uz-fkm-temp1` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-uz-fkm-temp2` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-uz-fkm-temp3` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-uz-fkm-temp4` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-uz-rkm-temp1` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-uz-rkm-temp2` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-uz-rkm-temp3` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-uz-rkm-temp4` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-uz-rkm-temp5` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-uz-rkm-temp6` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-index-g420-uz-rkm-temp7` | direct | 1 组 | 无 Python 计算器 | 通过 |
| `v3-keyway-keyway-footer` | direct | 依赖验收 | 依赖父模板 | 通过 |
| `v3-keyway-keyway-header` | direct | 依赖验收 | 依赖父模板 | 通过 |
| `v3-keyway-keyway-chamfer` | keyway.chamfer | 5 组 | 适用并通过 | 通过 |
| `v3-keyway-keyway-finish` | keyway.finish | 6 组 | 适用并通过 | 通过 |
| `v3-keyway-keyway-rough` | keyway.rough | 10 组 | 适用并通过 | 通过 |
| `v3-sync-turning-left-template` | sync.left | 4 组 | 适用并通过 | 通过 |
| `v3-sync-turning-right-template` | sync.right | 4 组 | 适用并通过 | 通过 |
| `v3-undercut-template` | groove | 68 组 | 适用并通过 | 通过 |

键槽 header 依赖粗铣、精铣、倒角，footer 依赖粗铣、精铣；全部父模板通过。普通模板没有 Python 计算器，其 Jinja 最终程序仍与冻结 V3 模板对照。主程序使用本次工序输出；V3 变量库中的手写 FINISH_TURNING / left_T_START3_END3 不作为历史程序默认，验收显式创建尺寸/终点/进给输入的新轮廓工序。实际零件需选择自己的轮廓工序。

## 可复现命令与本地证据

完整参数和隔离环境说明见 [迁移包 README](../../../examples/v3-managed/README.md)。Windows 设置 TEMP/TMP 指向仓库内临时目录，避免沙箱默认临时目录只读；浏览器设置 NCTOOL_BINARY 为上述固定文件，并通过 NCTOOL_PLAYWRIGHT/NCTOOL_CHROMIUM 使用独立浏览器依赖。

```powershell
npm run --prefix ui test
node scripts/build_ui.mjs
cargo fmt --all -- --check
cargo test --workspace --all-targets --all-features --locked --target-dir target/parameters-nc
cargo test --workspace --all-features --doc --locked --target-dir target/parameters-nc
cargo clippy --workspace --all-targets --all-features --locked --target-dir target/parameters-nc -- -D warnings
cargo build --workspace --features nc-bundle --locked --target-dir target/parameters-nc
cargo check --workspace --locked --target-dir target/parameters-template
cargo build --workspace --locked --target-dir target/parameters-template
python scripts/verify_managed_parameters.py --binary target/parameters-validation/nctool-complete2.exe
node scripts/browser_managed_parameters.cjs
node scripts/browser_v2.cjs
node scripts/browser_review_fixes.cjs
python -m unittest discover -s scripts/tests -p 'test_*.py'
python scripts/check_v2.py
python scripts/source_baseline.py verify .nctool-backups/parameters-20261007
python scripts/audit_dependencies.py --output target/parameters-validation/dependency-audit-final
git diff --check
```

本地明细位于 [CLI/HTTP 报告](../../../target/parameters-validation/comparison/report.json)、[浏览器报告](../../../target/parameters-validation/browser/report.json)、[Rust 日志](../../../target/parameters-validation/rust-tests-final.log)、[文档测试](../../../target/parameters-validation/rust-doc-final.log)、[clippy](../../../target/parameters-validation/clippy-final.log)、[审计摘要](../../../target/parameters-validation/dependency-audit-final/summary.json)。这些忽略目录不保证随包保留；validation.json 带逐模板明细、案例编号和指纹，可以随资产包交付。

## 限制与已记录告警

依赖审计首轮 npm 遇 TLS 网络错误，原报告保留在 `target/parameters-validation/dependency-audit`，未据此签收；随后重试 cargo/npm 均成功，漏洞计数为 0。仍保留 yoke-derive 0.8.3 被撤回的维护告警，没有升级、放宽版本或新增忽略。Rust 的 Windows 增量硬链接降级复制，以及 Vite 动态/静态导入和 Node 构建脚本弃用提示保留；它们未导致检查失败。

技术验收完成不等于控制器或机床工艺审核。源刀具缺失齿数、补偿号或材料牌号时明确待补，固定预置无需齿数时可用，必需事实不被猜测。本轮未执行此工作树的 Linux/macOS CI 或 Rust 1.89 MSRV，未提交、推送或发布。
