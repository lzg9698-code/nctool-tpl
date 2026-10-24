---
by: Codex GPT-6
date: 2026-10-24
---

# RustJinja 本地分支精简到 master

用户明确要求移除除主分支外的全部分支。本地主分支为 `master`，已在前序步骤快进到 `origin/master` 的 `ad0fbd8`（PR #40）。

删除所有其它本地分支引用前，将当前主工作区及 6 个链接 worktree 的检出分支切换为 detached HEAD；每个工作区的 HEAD 和脏项数量均保持不变。随后删除剩余全部非 `master` 本地分支。最终 `refs/heads` 仅有 `master`；工作区仍在各自原提交上，脏文件保留。当前主工作区 detached at `cba817c`，有 187 个 status 路径；其它 worktree 脏项数为 6、3、3、2296、3、5。

远端及 `origin/*` 跟踪引用未修改（28 条远端跟踪引用）。本轮未删除 worktree，也未删除文件。Git 元数据最初被沙箱限制为只读，随后受限提权完成操作。
