---
by: Claude Code
date: 2026-10-17
---

按用户“按路线图继续推进 P3”完成可准备的签收工作，未提交/推送/打标签/发布（按项目规则需要单独授权）。

产出 docs/P3_RELEASE_CANDIDATE_2026-10-17.md：候选身份（分支 feat/shared-parameter-inputs、HEAD 08ab296、工作树未提交）、关键指纹（cli/ui/source.sha256、Cargo.lock、package-lock、三份恢复归档 SHA-256）、本轮验证证据、发行契约、未完成项。

实际验证：前端 lint + 28 文件 188 项；Rust 默认与 math 各 282 项，连续 5 轮无失败；check_v2 通过；生成资源 14 文件一致；Python 33 项；三组浏览器回归通过；npm 0 漏洞；Rust 离线审计 0 漏洞 0 告警（公告库未刷新，本地库提交 64f92cdc，联网 cargo audit 300s 超时，按规则记为未完成）。默认/math release 构建成功（9,303,040 / 9,361,920 字节，SHA-256 已记录），隔离下载式冒烟验证 --version 2.2.0、actions 含通用动作无 NC、保存渲染、默认构建拒绝 math、math 构建需配置启用且显式 extensions（4|sqrt → 2.0）。

关键发现与修复：
1. 仓库根 node_modules/ 只含 vitest 缓存却未被忽略，补 /node_modules/ 规则。
2. 全量测试偶发失败（约 3 轮 2 次），单测独跑均通过。错误为 AlreadyExists(os 183) 与 os error 3：4 个测试夹具用 pid+纳秒命名临时目录，同进程并行线程可读到同一时钟刻度→同名目录→一方清理删掉另一方的目录。涉及 cli/tests/managed_parameters.rs、cli/tests/retirement.rs、plugins/parameters/tests/changed_group_lock.rs、plugins/template/tests/snapshot_lock_and_inline_root.rs；已加进程内 AtomicU64 计数。属测试基础设施缺陷，非产品并发缺陷。
3. 复核文档链接：当前文档除历史审查报告的 file:line（指向已删除 features/workbench/**）外无失效链接；已在该报告加“历史文件，当前仓库未保留”说明，不改写结论。

未完成：提交与推送、三平台远端 CI、标签与发行资产、下载后签收、联网刷新 RustSec 数据库、macOS/Linux 本机冒烟。另需决策：release.yml 只构建 --features math，是否同时提供默认组合二进制。本轮未改该工作流。
