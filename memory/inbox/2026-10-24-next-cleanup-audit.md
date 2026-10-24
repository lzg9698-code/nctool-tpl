---
by: Codex GPT-6
date: 2026-10-24
---

# 本地模型清理后的剩余空间审计

本轮只读盘点，没有删除文件。当前 C 盘约用 295.5 GiB、可用 256.5 GiB；D 盘约用 286.2 GiB、可用 113.8 GiB。

## 主要剩余占用

- D:\Dev 142.49 GiB，其中 D:\Dev\Projects 118.68 GiB，D:\Dev\Caches 23.81 GiB。
- D:\Program Files (x86) 62.04 GiB，其中 Steam 58.34 GiB，Counter-Strike Global Offensive 55.41 GiB。
- D:\AI\VMs\WSL\Ubuntu-22.04\ext4.vhdx 约 57.05 GiB。
- D:\Working 9.77 GiB，其中 Software 5.36 GiB，Backup 2.58 GiB；VERICUT 9.2.zip 约 4.55 GiB。
- D:\Program Files 8.81 GiB，含 CGTech 4.70、AutoCAD2026 1.60、VS Code 0.98、CIMCO 2025 0.95 GiB。
- C:\Users\Admin\AppData\Local\Temp 约 1.80 GiB；C:\Users\Admin\.cache 5.88 GiB，其中 Codex runtime 3.73、OpenCode 1.10、Sorana 0.75 GiB。
- Docker 报告：镜像 8.783 GiB，14/15 个 active，仅 71.76 MiB reclaimable；volume 1.198 GiB，报告 1.119 GiB reclaimable。
- WSL /home/lzg9698 当前约 39 GiB；`.cache` 约 6 GiB（pip 2.9、camoufox 1.4、Playwright 1.3）、`.npm` 0.93、`nctool-tpl/target` 1.5、`.nvm/.cache` 1.2 GiB。WSL 压缩前，内部删文件不保证立即释放 D 盘物理空间。
- D:\Dev\Caches 中 uv 5.91 GiB 清理仍被 in-use lock 阻止；pnpm store 7.97 GiB 含 v10\projects 的项目内容，不应整体删除。Rustup 5.23 GiB 属工具链。
- RustJinja Windows target 约 63.7 GiB、worktree 合计约 48.7 GiB；此前 clean_targets 预览为 0 reclaimable，worktree 也含脏改动，应待工作完成后重审。

## 清理建议

- 最大的用户可选项是通过 Steam 卸载不再玩的 CS:GO，约 55.4 GiB；不要手动删除安装目录。
- 温和候选包括 Windows Temp（约 1.8 GiB）、Docker volumes（工具报表约 1.119 GiB 可回收）、WSL 的包缓存（约 6 GiB）和 Rust 构建产物；都需要进一步确认进程与数据用途。
- 软件安装目录、备份、项目工作树、Codex runtime、Rustup、WSL VHDX 保留，除非用户指定具体项。
