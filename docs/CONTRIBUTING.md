# 贡献指南

> 面向在本仓库上做二次开发 / 提 PR 的开发者。README 中的「贡献指南」是本文速览。
> 相关：[ARCHITECTURE.md](ARCHITECTURE.md)（架构）· [ROADMAP.md](ROADMAP.md)（阶段计划与 Backlog）·
> [RELEASE.md](RELEASE.md)（版本与发版）· [PROCESS_CHECKLIST.md](PROCESS_CHECKLIST.md)（工艺安全边界）

---

## 1. 前置阅读（必读）

1. **README 开头的「定位与风险声明」** —— 本项目是**模板开发工具**，内置模板与机床预设
   **未经真实工艺评审与机床空运行验证**。G/M 代码错误可能导致撞刀、刀具或设备损坏。
2. [ARCHITECTURE.md](ARCHITECTURE.md) —— 三层 crate 架构、模块职责、数据流、错误模型、扩展点。
3. [ROADMAP.md](ROADMAP.md) —— 当前阶段、任务清单与 Backlog 排序，避免方向冲突或重复劳动。
4. [TEMPLATE_WRITING_GUIDE.md](TEMPLATE_WRITING_GUIDE.md) —— 改模板前必读（必选/可选判定、过滤器、反模式）。

> ⚠️ **工艺正确性问题不属于工具 bug**，不按 issue 流程处理，请走
> [PROCESS_CHECKLIST.md](PROCESS_CHECKLIST.md) 与外部工艺评审。

---

## 2. 环境搭建

| 项目 | 要求 |
| --- | --- |
| Rust | **1.85+**（MSRV = workspace 各 crate 的 `rust-version`）。CI 在 stable 上跑质量门，另有 `msrv` job 在 1.85 上 `cargo check --workspace --locked`，让这个承诺可验证。**抬 MSRV 前先看该 job**：1.82 时代它是红的（`clap_derive` 需要 `edition2024`），本机 stable 不会暴露这种问题 |
| 组件 | `rustfmt`、`clippy`（CI 用 `dtolnay/rust-toolchain@stable` 安装） |
| 可选工具 | `cargo-audit`（安全审计）、`cargo-llvm-cov` + `python`（覆盖率，CI **阻断**项） |
| 平台 | Linux / macOS / Windows 均需可用（CI 三平台矩阵） |

```bash
git clone https://github.com/lzg9698-code/nctool-tpl.git
cd nctool-tpl

cargo build --workspace
cargo test --workspace
cargo run -p nctool-cli -- templates list    # 不安装、直接跑 CLI
cargo run --example demo                     # 库的可运行示例

cargo install cargo-audit --locked           # 安全审计
```

### 仓库结构

| 路径 | crate / 内容 | 职责 |
| --- | --- | --- |
| `src/` | `nctool-tpl` | 模板解析 + 变量提取（可选/必选）+ 渲染（NC / 数学过滤器） |
| `core/` | `nctool-core` | 数据模型 + 参数校验 + 模板注册表 + 机床配置 + G-code 生成管线 |
| `cli/` | `nctool-cli` | `nctool` 命令行（薄壳）+ Web UI（`cli/ui/index.html`） |
| `tests/` | 根 crate 集成测试 + `tests/golden/` 基线 | 解析、渲染、golden 逐字节比对 |
| `benches/`、`core/benches/` | criterion 基准 | 解析/提取/渲染、后处理与端到端管线 |
| `docs/` | 设计、指南、路线、状态 | 改架构/配置/工艺必须同步更新 |

---

## 3. 质量门（与 CI 完全一致，提交前必须全绿）

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test --workspace --doc          # --all-targets 不跑 doctest，CI 为此单列一步
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
node scripts/check_param_parity.mjs   # --param 归一规则的 Rust / 前端对拍
cargo audit
```

> **`--workspace` 不能省**：根目录既是 workspace 根又是一个 package，cargo 在没有
> `default-members` 时默认只选根 package。漏掉它，三条 cargo 命令就只对
> `nctool-tpl` 生效，`core` / `cli` 的测试与 lint 会被静默跳过——质量门照样全绿，
> 实际上什么都没查。

CI（`.github/workflows/ci.yml`）在 **ubuntu / windows / macos** 三平台各跑一遍上述 cargo 检查
（对拍脚本与系统无关，只在 ubuntu 跑一次），全部必须绿灯；
`coverage` 与 `msrv` 两个 job 同样是**阻断**项，带**阈值门**与**MSRV 门**：

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov        # 本地复现 CI 覆盖率门需要这两步
cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info
python scripts/check_coverage_caliber.py lcov.info --min 89
```

阈值是**生产代码**行覆盖 ≥ 89%（2026-09-19 由 88% 上调），当前基线 **90.34%**（Ubuntu CI 实测；本机 Windows 数字会略低，以 CI 为准）。

> **不要用 `--fail-under-lines`。** llvm-cov 把 `src/*.rs` 内的 `#[cfg(test)]` 段本身
> 也计入分母（本仓库 `src/lib.rs` 1824 行里测试段占 1760 行），于是「新增测试」会
> **推高**覆盖率数字、「新增未覆盖的生产代码」反被稀释 —— 原始口径 94.04%，而剔除
> 测试段后的生产代码只有 90.34%（2026-09-19 实测），**门禁绿 ≠ 生产代码达标**。
> `--ignore-filename-regex` 只能按文件路径排除，管不到 `src/` 内部的测试段，故改由
> `scripts/check_coverage_caliber.py` 从 lcov 数据剔除测试段后重新统计。该脚本会
> 同时打印两种口径的数字，便于核对。
>
> 要分析 **CI 产物**（而不是本地生成的 lcov）时加 `--strip-prefix`：runner 上记录的
> 是绝对路径（`/home/runner/work/<repo>/<repo>/cli/src/args.rs`），本地没有该路径，
> 剥掉前缀才能映射到本仓库。可在 CI run 的 Artifacts 里下载 `rust-coverage-lcov`：
>
> ```bash
> python scripts/check_coverage_caliber.py lcov.info \
>     --strip-prefix /home/runner/work/nctool-tpl/nctool-tpl/
> ```

余量约 68 行未覆盖生产代码（阈值 89% vs 实测 90.34%）。**刻意不贴着实测值设阈值**：
余量只剩十几行时任何一次小改动都可能误触，而"经常误报的门禁会被当成噪音忽略"。
**覆盖率提升后请上调这个数字** —— 只改 `ci.yml` 里 `python3 scripts/check_coverage_caliber.py
lcov.info --min 89` 那一行的 `--min`，一处。
门禁失败时 job summary 与 lcov 产物仍会产出（那两步带 `if: always()`）——
排查"覆盖为什么掉下去"正需要它们。

已知平台问题：

- **Windows 打包 ICE**：rustc 1.98 在打包阶段可能触发 ICE，用
  `CARGO_INCREMENTAL=0 cargo package --workspace --allow-dirty` 规避。
- **行尾与配置路径**：golden 比对已做 `normalize_newlines`；跨平台的配置目录差异
  （`XDG_CONFIG_HOME` 等）请在测试里显式设置环境变量，不要依赖宿主环境。

文档改动后顺手跑一次链接自检（无第三方依赖，Python 3.8+）：

```bash
python scripts/check_docs_links.py README.md docs
```

它检查 Markdown 里的相对链接文件是否存在、heading 锚点是否可解析（当前 `docs/` 全量已通过）。

---

## 4. 测试矩阵

`cargo test --workspace` 覆盖下列目标。**具体项数不在此处硬编码** —— 它每加一个测试
就过期一次，本文档曾因此同时漂移出 344 / 516 / 492 三个互相矛盾的版本。以 CI run 的
job summary 为准：

| 目标 | 类型 |
| --- | --- |
| `nctool-cli` | 单元测试（bin） |
| `cli/tests/cli.rs` | 集成（HTTP 契约等） |
| `cli/tests/cli_e2e.rs` | CLI 端到端（退出码契约） |
| `nctool-core` | 单元测试 |
| `core/tests/integration.rs` | golden 集成 |
| `core/tests/large_program.rs` | 万行性能（2 项 `#[ignore]`） |
| `nctool-tpl` | 单元测试 |
| `tests/extract_invariant.rs` | 提取器属性测试（300 例，零依赖 LCG） |
| `tests/parsing.rs` | 根 crate 集成 |
| doc-tests | 文档示例（`cargo test --doc`；注意 `--all-targets` **不跑** doctest） |

万行级实测（默认跳过，需 release）：

```bash
cargo test --release -p nctool-core --test large_program -- --ignored
```

性能基线（改动渲染/后处理路径后请复跑）：

```bash
cargo bench                                   # 解析 / 提取 / 渲染
cargo bench -p nctool-core --bench pipeline   # 后处理与端到端管线
```

### golden 基线

G-code 输出是逐字节比对的（`tests/golden/`，42 个基线文件）。

```bash
NCTOOL_UPDATE_GOLDEN=1 cargo test --workspace # 刷新基线（必须 --workspace：golden 在 core 包）
git diff tests/golden                         # 必须人工逐行复核
```

**禁止**用刷新基线掩盖非预期回归 —— 每次刷新都要在 PR/commit 里说明「为什么输出变了」。

---

## 5. 对外稳定契约（改动需格外谨慎）

以下都是被外部脚本 / 前端 / 测试依赖的契约，变更必须同步更新测试与 CHANGELOG：

| 契约 | 位置 | 说明 |
| --- | --- | --- |
| CLI 退出码 0–7 | README「退出码」表 | 由 `cli/tests/cli_e2e.rs` 44 个用例逐条断言 |
| `--format json` 输出结构 | `cli/src/output.rs` | 成功 `{"ok":true,"data":…}`，失败 `{"ok":false,"error":…}` |
| Web UI HTTP API | `cli/src/server.rs` + `ui/index.html` | 契约端点与字段名 |
| MSRV 1.82 | 各 crate `Cargo.toml` 的 `rust-version` | 提升需改 CI 并在 CHANGELOG 标注 |
| `minijinja ~2.24.0` | 根 `Cargo.toml` | 依赖 `unstable_machinery` / `debug` feature，升 minor 可能编译不过，需全量验证 |

### 改动同步清单

| 改了什么 | 必须同步 |
| --- | --- |
| 公共 API / 错误类型 / 行为 | `CHANGELOG.md` 的 `Added` / `Changed` 节 + 相关文档；破坏性变更附迁移方式 |
| CLI 退出码或 JSON 字段 | `cli/tests/cli_e2e.rs` + CHANGELOG + README 退出码表 |
| G-code 输出字节 | golden 基线（人工复核）+ CHANGELOG |
| 架构 / 模块职责 / 数据流 | `ARCHITECTURE.md` |
| 机床配置键 | `MACHINE_CONFIG_GUIDE.md`（键清单、未知键告警） |
| 模板写法 / 过滤器 | `TEMPLATE_WRITING_GUIDE.md` |
| 新增内置模板 | golden 用例 + `PROCESS_CHECKLIST.md` 登记 + 声明「未经工艺评审」 |

---

## 6. 提交与评审

- **主分支**：`master`。
- **commit message**：中文或英文均可，必须含**动机**（why），不是只写「改了 X」。
  每条 commit 应**自包含**（独立编译、独立测试通过），便于 `git bisect` / `git revert`。
- **0.x 阶段**：master 直推 + 事后 review；**1.0 后**启用 PR 流程：
  1 个 approve（维护者本人可省）+ 三平台 CI 全绿；有破坏性变更时 PR 描述必须写迁移指南。
- **PR 描述**：使用仓库模板 [`.github/PULL_REQUEST_TEMPLATE.md`](../.github/PULL_REQUEST_TEMPLATE.md)
  （含质量门、文档同步与工艺安全核对项）。
- **评审关注点**：正确性（尤其边界与错误路径）> 一致的错误处理 > 测试覆盖 > 文档同步 > 风格。

---

## 7. 发版

标签即发布开关（`.github/workflows/release.yml`）：

```bash
# 推送形如下列的 tag 即触发：nctool-tpl-v* / nctool-core-v* / nctool-cli-v*
git tag nctool-cli-v0.3.0 && git push origin nctool-cli-v0.3.0
```

- 会跑 `cargo test --workspace`，然后 `cargo publish -p <crate>`（需仓库 Secret `CARGO_REGISTRY_TOKEN`，
  未配置时跳过并提示）；同时构建 Linux / Windows / macOS（arm64）三平台二进制并上传为 Release assets。
- 版本号遵循 SemVer；0.x 阶段 minor 可含破坏性变更（需 CHANGELOG 标注 + 迁移说明）。
  三个 crate 通常一起 bump，允许 core/tpl 落后 cli 一次。详见 [RELEASE.md](RELEASE.md)。

---

## 8. 报告问题

- **Bug / Feature** → [issue 模板](https://github.com/lzg9698-code/nctool-tpl/issues/new/choose)（两类，`blank_issues_enabled: false`）。
- **用法 / 配置 / 模板写法** → [Discussions](https://github.com/lzg9698-code/nctool-tpl/discussions)。
- **工艺 / G 代码安全性** → [PROCESS_CHECKLIST.md](PROCESS_CHECKLIST.md)，不当作工具 bug 处理。

---

## 9. Reviewer 检查清单

- [ ] `fmt` / `clippy -D warnings` / `test` / `doc -D warnings` / `audit` 全绿
- [ ] 新增或修改的行为有对应测试（含失败路径与边界）
- [ ] 若输出字节变化：golden 已刷新且差异已人工复核、原因已说明
- [ ] 若触碰退出码 / JSON / HTTP 契约：测试与文档同步，CHANGELOG 已登记
- [ ] 文档同步（ARCHITECTURE / 配置指南 / 模板指南 / README / CHANGELOG）
- [ ] 模板与机床相关改动已标注「未经工艺评审」，未宣称可用于投产
