---
by: Codex GPT-6
date: 2026-10-24
---

# Steam 与 Rust 编译产物清理

用户要求完整移除 Steam，并补充清理 Rust 编译产物。

## Rust 产物

- 删除 3 个 worktree target（21.07、14.64、11.13 GiB）、一个 UI target（1.32 GiB）、`3ways` Tauri target（2.16 GiB）和 pnpm store 项目中的 Tauri target（3.33 GiB）。
- RustJinja 主 target 为 63.70 GiB。确认没有 Cargo/rustc 编译进程后删除 59 个未被进程引用的直接配置目录；保留运行中 `nctool.exe` 的 `engineering-math` 和运行中 `ftu-plugin.exe` 的 FTU 验收配置。剩余 target 约 1.56 GiB。
- WSL `/home/lzg9698/nctool-tpl/target` 约 1.5 GiB，通过 `cargo clean` 清理；Git 源码状态前后为空。
- 源码与其他脏 worktree 文件保持；restore-shared-parameters worktree 的状态计数从 2296 降到 88，因为删除了 untracked target 输出。
- 用户随后要求停止本项目所有 Rust 相关进程并清除主 target。按项目路径筛选并停止 `nctool.exe`、其 `ftu-plugin.exe` 子进程及 `conhost.exe`，删除剩余 `D:\Dev\Projects\nctool\rustjinja\target`（最后约 1.56 GiB）。当前 worktree Git 状态计数前后均为 191，D 盘可用约 280.6 GiB。
- 用户确认只保留 `master` 和当前工作分支。将当前 detached HEAD `cba817c` 恢复为跟踪 `origin/codex/release-v2.3.0-record` 的本地分支，并按精确路径移除其余6个脏 linked worktree；主工作区状态计数保持 191，远端分支未修改。最终本地分支仅 `master` 和 `codex/release-v2.3.0-record`，仅保留主 worktree。

## Steam

- Winget 卸载 `Valve.Steam` 成功；Steam 根目录、CS2、Wallpaper Engine 游戏内容及 Steam 用户缓存/快捷方式/用户级 Valve/Steam 配置已移除。
- D 盘可用空间最后测得约 279 GiB。
- 删除剩余 HKLM 卸载登记和 Windows Firewall Steam/Steam Web Helper 规则时被 Windows 拒绝，错误为 Requested registry access is not allowed。当前仍有 Counter-Strike 2、Wallpaper Engine 两条机器级卸载登记和 4 条 Steam 防火墙规则；需要管理员令牌才能清理。远端 Steam Cloud 数据未修改。
