---
by: Codex GPT-6
date: 2026-10-24
---

# D 盘开发文件空间盘点

用户请求梳理 D 盘占用并准备清理方案。本轮只读扫描，没有删除、移动或重命名文件。

## 当前证据

- D 盘约 400 GiB，已用 391.5 GiB，可用约 8.5 GiB。
- 使用 `D:\ai-brain\dirsize.ps1` 只读扫描：逻辑文件总和约 403.07 GiB；与卷已用量差异可能来自硬链接等重复计数，卷空间统计作为容量依据。
- 顶层主要占用：`D:\Dev` 208.24 GiB、`D:\AI` 102.68 GiB、`D:\Program Files (x86)` 62.04 GiB。
- 开发目录中：`D:\Dev\Projects` 174.04 GiB；当前 RustJinja `target` 63.70 GiB，RustJinja `.claude\worktrees` 48.69 GiB；`nctool-next\target` 55.26 GiB。另有 `D:\Dev\Caches` 34.29 GiB，其中 pnpm-store 8.49、uv 5.55、rustup 5.23、npm 4.16、pip 2.72、Nuitka 2.60、Yarn 1.70 GiB。
- `D:\AI` 约 57.05 GiB 是 Ubuntu WSL `ext4.vhdx`，约 45.63 GiB 是 Ollama 模型；应作为用户数据，未经确认不删除。
- RustJinja 主工作区有大量未提交改动；6 个关联 worktree 均有脏文件，且分支/提交不同。主仓库多个 target profile 今天仍有更新。
- 执行 `python scripts/dev/clean_targets.py --json` 只读预览，结果为 `candidates: []`、`reclaimable: 0`；脚本因目录近期更新或不能确认活动进程而保守保留所有 target 子目录。
- `D:\ai-brain\disk-report.md` 和 `scan-D.csv` 最后更新于 2026-10-11，旧数据已明显过时；例如现有 Downloads 与回收站很小。

## 清理方向

优先审查 Cargo 构建目录，确认无构建/验收进程后再逐目录评估；保留工作树、分支、未提交改动，只考虑可重建的 target 内容。随后再评估包管理器缓存，先用包管理器 prune/clean 并检查 pnpm-store 中包含的项目路径。暂缓清理 WSL 磁盘、模型、安装目录和工程归档。

## 遗留

RustJinja target 的预览没有可清理候选；执行其他清理前仍须核实目录配置、活动进程与实际回收量。初始盘点时 `nctool-next` Git 状态读取受沙箱限制，之后在用户明确要求删除后完成核验并移除了该项目。

## 后续执行

- 用户明确要求彻底删除 `D:\Dev\Projects\nctool\nctool-next`。删除前核对该目录是普通目录、Git 工作区干净、无远端、无子模块、只有一个本地 `main` 分支且无额外 worktree；删除后路径不存在，D 盘可用空间由约 8.5 增至 57.1 GiB。
- 后续清理核对发现 npm 活动缓存位于 `D:\Dev\Caches\npm`。先因存在 Node 进程中止；确认无 npm/pnpm/yarn 安装或缓存操作进程后，执行 `npm cache clean --force` 和 `npm cache verify`。`_cacache` 清空、0 项，D 盘可用空间增至 59.2 GiB；仍保留约 2.06 GiB 的 `_npx` 安装缓存目录，未清理。
- 最初当前 PowerShell 会话显示 uv、pip、Yarn 的缓存路径在 C 盘；随后检查用户级环境变量发现这些缓存目录实际配置在 D 盘。pip 与 Yarn 的 D 盘缓存已分别清理；Cargo/Rustup、NuGet 和 Playwright 浏览器保留未动。
- 随后使用本机 pnpm 10.33.0 对活动 store `D:\Dev\Caches\pnpm-store\v10` 运行非强制 `pnpm store prune`，移除 42,198 个文件和 775 个包；D 盘可用空间增至 60.1 GiB。store 中仍有 3.46 GiB 的 `v10\projects`，保留未删。
- 核实 Yarn 当前 cache 配置在 C 盘，删除未被其它进程引用的旧 D 盘副本 `D:\Dev\Caches\yarn`（1.70 GiB）；D 盘可用空间增至 61.8 GiB。
- 通过临时 `PIP_CACHE_DIR` 指向用户级配置 `D:\Dev\Caches\pip`，执行 `python -m pip cache purge`；移除约 2.92 GB/2.72 GiB 的 4,884 个缓存文件，无持久环境变量变更。
- 删除未被活动进程引用的 `D:\Dev\Caches\nuitka`（2.60 GiB）和 Electron 下载缓存 `D:\Dev\Caches\electron`（约 0.62 GiB）。
- `pnpm cache delete '*'` 未清除条目；检查确认剩余 `pnpm-cache\v11` 仅含 registry 元数据且无活动 pnpm 操作，删除精确缓存目录（约 0.30 GiB）。
- uv cache clean 对 `D:\Dev\Caches\uv` 检测到使用中锁并等待；未使用 `--force`，中止后保留该 5.91 GiB 目录。D 盘最终可用约 68.1 GiB。npm 的 `_npx` 2.06 GiB 与 pnpm `v10\projects` 3.46 GiB 仍保留。
