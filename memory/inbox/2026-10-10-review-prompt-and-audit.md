---
by: opencode / mimo-v2.6-flash-free
date: 2026-10-10
---

会话摘要：应用户要求「写一个提示词审查本项目」，先创建 `docs/REVIEW_PROMPT.md`（基于 AGENTS.md、`$nctool-review` Skill 与架构文档提炼的可粘贴全面审查提示词），随后用该提示词对当前未提交改动执行了审查。

本轮审查范围：`docs/README.md`（用户改动）、`docs/NC_FAMILY_GENERATION_PROPOSAL.md`、`prototypes/dual_turret/`、`scripts/tests/test_dual_turret_prototype.py`、未跟踪的 `nc/` 与 `memory/`。Rust/前端源码未改动。

主要发现（按影响）：
1. 高：新测试依赖未跟踪的 `prototypes/` 与 `nc/`，而 CI `platform` 作业在三平台运行 `python -m unittest discover`。仅提交测试文件会在干净 clone 上 import 即失败（已用 `git archive HEAD` 副本复现）。`git add -A` 则会把 `nc/`（214 文件 / 5.2MB 真实程序与 22 张图纸）推上 PUBLIC 仓库。
2. 中：`.gitattributes` 的 `* text=auto eol=lf` 会把 CRLF 的 `*.MPF` 规范化为 LF，使 `read_source` 记录的 SHA-256 证据在新 clone 上失效。
3. 中低：提案中 6 处 `../nc/...` 相对链接在公共 clone 上悬空。
4. 低：`prototypes/dual_turret/README.md:7` 含本机用户路径 `C:/Users/Admin/.codex/...`。

已验证：10 项原型测试与 24 项 Python 工具测试通过；`check_v2.py` PASS；CLI 失败/成功/拒绝覆盖三态正确；提案对源码的断言（通道上限 1..=2、`WAITM(number,1,2)`、Schema `maximum:2`）与实现一致；提案的样本计数（11 套程序 / 16+6 图纸 / 5 个无同号程序）与目录实际一致；16 个本地链接目标存在。

遗留：仓库 PUBLIC 与全局「保持私有」规则的冲突属既有状态，是否提交 `nc/` 需维护者决策；本轮未运行 Rust/浏览器全量验证（改动未触及）。本轮仅审查，未修改业务源码，未提交。

---

用户随后授权「按报告修复问题」，本轮已完成以下修复（均在本地，未提交）：

1. `.gitignore` 新增 `/nc/` 与 `/memory/`（附注释），防止 `git add -A` 把真实机床程序/图纸与 AI 会话笔记推上公共仓库。
2. `scripts/tests/test_dual_turret_prototype.py`：新增 `requires_samples`（`unittest.skipUnless`），给两项依赖 `nc/` 的测试加装饰器，并将其改为使用 `NC_SAMPLES` 常量。干净检出（无 `nc/`）下整套 Python 测试由 import 失败变为 `OK (skipped=2)`、exit 0。
3. `docs/NC_FAMILY_GENERATION_PROPOSAL.md`：把 7 处指向未跟踪文件的相对链接（`../nc/` 6 处、`../templates/` 1 处）改为本地路径代码样式并注明「不随仓库分发」；§2 说明 `nc/` 属仓库外资料。
4. `prototypes/dual_turret/README.md`：移除本机用户路径 `C:/Users/Admin/.codex/...`；补充 `nc/` 缺失时样本测试自动跳过的说明。

验证：本地（有 nc/）10 项原型测试通过；`git archive HEAD` 干净副本 + `prototypes/` + 新测试（无 `nc/`）→ 24 项运行、2 跳过、exit 0；`python scripts/check_v2.py` PASS；提案剩余相对链接目标均为已跟踪文件；`git check-ignore` 确认 nc/、memory/ 已忽略。未提交。

