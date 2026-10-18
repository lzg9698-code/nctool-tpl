# P3 发行候选与签收准备（2026-10-17）

> **发布结果（2026-10-18）**：已发布 [v2.2.0](https://github.com/lzg9698-code/nctool-tpl/releases/tag/v2.2.0)，标签指向 `1443046`。
> 发布工作流 [39090327111](https://github.com/lzg9698-code/nctool-tpl/actions/runs/39090327111) **全绿**：
> 三平台 CI + msrv + dependency-audit 通过，三平台二进制构建成功，自动发布。
>
> | 资产 | SHA-256 |
> | --- | --- |
> | `nctool-linux-x86_64` | `acc944ba4b3761fbfd09b408b590016d0a6c02435a0f82852f30744ece2589cc` |
> | `nctool-macos-arm64` | `ff90e72a844e08884680a1a82b2d4b61594023bdc2a07c8adb8999e969ac209d` |
> | `nctool-windows-x86_64.exe` | `4f65c1a75c38ca7d2c3eef26e95d371ad346392997ef77085facd14f68144648` |
>
> 各平台资产均通过对应 `.sha256` 校验；Windows 产物在隔离 home/workspace 下通过
> `--version`、`actions`、保存与渲染、默认拒绝 `math`、显式启用后 `4 | sqrt → 2.0` 的冒烟。
>
> 首次发布（标签指向 `97a4d1c`）的 publish 作业失败：下载了全部工件，把 `dependency-audit`
> 的空 `*.stderr.txt` 一并上传，被 GitHub 拒绝。工作流已修复（`df56489`）；把标签移到含修复的
> `1443046` 后重跑，得到上述全绿记录。

本页整理当前工作树作为**可审阅的发行候选**，并记录本轮实际完成与未完成的签收项。

## 1. 候选身份

| 项 | 值 |
| --- | --- |
| 分支 | `master` |
| 发布标签 | `v2.2.0` → `1443046` |
| 功能提交 | `1918dfb`（PR #31） |
| 评审修复 | `5f4ee56`、`39dcea7`（PR #33） |
| 发布工作流修复 | `df56489` |
| 发布与签收文档 | `1443046` |

本节记录的是候选整理时的起点；最终发布以标签 `v2.2.0` 指向的提交为准。候选整理前的工作树含大量未提交改动，已按主题提交并合并，工作树现为干净状态。

## 2. 候选范围

- 产品范围：通用模板 + 共享参数 + 通用插件运行时；`math` 为唯一可选 feature；内置 NC/机床/工序/同步粗车/越程槽/轨迹已移除。
- P1：入门示例（普通文本/共享参数）、模板编辑的共享绑定作者面板。
- P2：`App.tsx` 职责拆分、主样式模块化、历史检索分页与单条删除、性能测量工具。
- 后续：参数管理页改为列表 + 结构化编辑；共享绑定与使用端输入分区；修复表单失效、字段对齐与内边距。
- 回归脚本：`browser_review_fixes.cjs` 的偶发失败已定位为测试对 `<details>` 的盲目切换（非产品缺陷），改为幂等展开并等待编辑内容就绪。

## 3. 关键指纹

| 对象 | 值 |
| --- | --- |
| `cli/ui/source.sha256` | `a1c8bce9636227a28fba2df77a90c80a657450504787ee485051a610affc0f71` |
| `Cargo.lock` SHA-256 | `9853fb0f8746ccaa2480aa293ab4bebc7367a1550e75c7d1ba703d33d82c79c5` |
| `ui/package-lock.json` SHA-256 | `d026b17ba4a716dfba35a6bf839a0f1249131651f04d361b05fcf22d4bc1d133` |
| 生成资源一致性 | `node scripts/build_ui.mjs --check` 通过（14 文件） |

恢复归档保持原字节，未在本轮改写：

| 归档 | 字节 | SHA-256 |
| --- | ---: | --- |
| `docs/archive/legacy/rust-nc-removal-20261016.tar.gz` | 811,113 | `60e467e62f2ab4cef19aea42d0b23bd7bc72b67b7e2e8c4d5ad210dfd1cfc5c5` |
| `docs/archive/legacy/rust-nc-workbench.tar.gz` | 881,718 | `8837a6fa9bb1167dc8558c45bd6f25a2fc9b16390f59aca5d27adf1fd30ace35` |
| `docs/archive/legacy/workbench-fixed-scheme.tar.gz` | 1,123,161 | `35f97872ab6e81885e4f6694722e80ac743fe8c06c474dd22ed80c15d64be7be` |

## 4. 本轮实际验证（Windows 11）

| 项 | 结果 |
| --- | --- |
| 前端 lint（tsc 类型与未使用声明） | 通过 |
| 前端测试 | 28 文件 188 项通过 |
| Rust 默认组合（`--workspace --all-targets`） | 282 项通过，连续 5 轮无失败 |
| Rust math 组合（`--all-features`） | 282 项通过，连续 5 轮无失败 |
| `cargo fmt --all -- --check` | 通过 |
| 架构与包内容 `check_v2.py` | 通过 |
| 生成资源一致性 | 通过（14 文件） |
| 主流程浏览器回归 | 通过 |
| 共享参数 / math 浏览器回归 | 通过 |
| 输入边界浏览器回归 | 通过（修复后连续 6 次） |
| 依赖审计（npm） | 0 漏洞 |
| 依赖审计（Rust） | 0 漏洞、0 告警；**离线运行，公告库未刷新**（本地库提交 `64f92cdc`） |
| 默认 release 构建 | 通过 |
| math release 构建 | 通过 |

### 发行冒烟（模拟下载后使用）

二进制复制到全新目录，使用临时 `--home` / `--workspace`：

| 检查 | 结果 |
| --- | --- |
| `--version` | `nctool 2.2.0` |
| `actions` | 含 `template.render`、`template.render.managed`、`parameter.resolve`；无 NC 相关动作 |
| 保存并渲染模板 | 通过，输出 `Hello NCtool` |
| 默认构建请求 `math` 扩展 | 拒绝（退出码 1） |
| math 构建未启用配置时请求 `math` | 拒绝（退出码 1） |
| math 构建启用配置且显式选择 `extensions` | 通过，`{{ 4 | sqrt }}` → `2.0` |

### 顺带修复：测试临时目录重名

全量运行会偶发失败（约 3 轮出现 2 次），单独运行同一测试却通过。实际错误是 `AlreadyExists`（os error 183）与 `os error 3`（找不到路径）：4 个测试夹具用 `进程 ID + 纳秒时间戳` 命名临时目录，同一测试进程内的并行线程可能读到同一时钟刻度而得到同名目录；一方结束时的清理删除目录，另一方随即失败。

涉及 `cli/tests/managed_parameters.rs`、`cli/tests/retirement.rs`、`plugins/parameters/tests/changed_group_lock.rs`、`plugins/template/tests/snapshot_lock_and_inline_root.rs`；已按 `cli/tests/history_management.rs` 的既有写法加入进程内 `AtomicU64` 计数。**这是测试基础设施缺陷，不是产品并发缺陷**——修复前单独运行每个测试均稳定通过。修复后默认与 math 各连续 5 轮全绿。

### 本机产物

| 构建 | 字节 | SHA-256 |
| --- | ---: | --- |
| 默认 release | 9,303,040 | `499ab012e3b1da81aee1d4a8933e28ba9feb51f043f2f992aff61a7d667a8dc4` |
| math release | 9,361,920 | `dafe7f3489f5f168f46e221501d8c6311d3c1735013c3fbec978a6561b54b870` |

它们是**本机 Windows 构建**，不是发行资产，也不代表其它平台。

## 5. 发行契约（现有工作流）

`.github/workflows/release.yml` 在推送 `v2.*` 标签时：

1. 复用 `.github/workflows/ci.yml` 作为前置门槛，任一作业失败则不构建、不发布。
2. 构建三平台二进制：`x86_64-unknown-linux-gnu`、`x86_64-pc-windows-msvc`、`aarch64-apple-darwin`。
3. 为每个二进制生成同名 `.sha256`。
4. 通过 GitHub Release 发布，自动生成发行说明。

**需要决策的差异**：工作流只构建 `--features math` 的二进制，不单独产出默认组合产物。若发行需要同时提供默认与 math 两份二进制，需先改工作流；本轮未改动。

## 6. 签收状态

| 项 | 状态 |
| --- | --- |
| 提交候选源码 | 已完成：`1918dfb`（PR #31），评审修复 `5f4ee56`、`39dcea7`（PR #33） |
| 三平台远端 CI | 已完成：Linux / Windows / macOS + msrv + dependency-audit 全部通过 |
| 依赖审计数据库刷新 | 已完成：公告库更新到 `cc76a4d5`，cargo 0 漏洞 0 告警、npm 0 漏洞 |
| 标签与发布 | 已完成：`v2.2.0`，指向 `1443046`，发布工作流全绿 |
| 发行资产与下载后签收 | 已完成：三平台资产 SHA-256 校验一致，Windows 产物冒烟通过 |
| macOS / Linux 实机冒烟 | **未执行**：本机为 Windows，仅验证了 CI 构建与 Windows 产物 |

发布工作流本身已修复（`df56489`），后续发标签不会再带入审计报告。

## 7. 后续注意

1. 发行只包含 `--features math` 二进制，不单独提供默认组合产物（已确认的产品取舍）。
2. Windows 产物在两次构建中得到不同 SHA-256（代码未变），说明该平台构建不是逐字节可复现的；完整性以发布时附带的 `.sha256` 为准。
3. macOS / Linux 的实机冒烟仍未执行，只有 CI 构建与哈希校验。
5. 打 `v2.*` 标签并发布，下载三平台产物做 SHA-256 与冒烟核对。
6. 在联网环境刷新 RustSec 数据库后重跑审计并保留报告。
