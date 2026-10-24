---
by: codex-gpt-6
date: 2026-10-23
---
按审查报告修复：external transport 满容量且所有请求 abandoned 时 fail/杀进程，使后续握手恢复；保留活动 sibling。新增永久无回复两轮超时恢复和取消恢复测试，调整旧容量测试时序并断言 sibling 成功。Vite 精确升级7.3.7，仅该包锁项变化，构建仅source.sha256变化。全feature workspace全目标测试通过；最终外部协议11项通过；前端210项、lint/build、架构、Clippy、browser_v2通过。未提交推送。审计最终结果见target/fix-audit。

最终安全复核：cargo --no-fetch 使用本轮已更新公告库无漏洞无警告；npm audit 0漏洞。统一脚本结果见target/fix-audit/summary.json。
