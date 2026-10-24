---
by: Codex GPT-6
date: 2026-10-24
---

# 原生工作台与 FTU / Jinja 实现交付

- 用户授权实施已确认计划，并要求继续。已在原工作树实现原生工程工作台、项目/草稿/冲突/回滚、预览并发、正式任务冻结、通用场景与文本产物 ZIP 下载。
- FTU 为 external/ftu 独立 Rust crate，独立 Cargo.lock，不加入默认 workspace；使用模板服务版本 1 / 进程协议 2，提供 analyze/prepare/template/generate，严格数值格式与渲染后块语义验证。原 FTU 未修改，仅只读对照。
- 新增声明式 nctool.package.json；无加工语义的 engineering-demo 也复用底座。使用与本地交付记录见 docs/ENGINEERING_WORKBENCH.md、docs/archive/records/ENGINEERING_FTU_IMPLEMENTATION_2026-10-24.md。
- 本机通过：Rust workspace 307 passed/1 ignored，FTU 8，前端 47（threads 模式），Python 60；Clippy/fmt/MSRV1.89/doctests、架构/资源检查；统一三组浏览器30项、工程11项、协议6项及原FTU16组合差分。
- 500特征/100禁入区/1004元素，四格式917162字节，本机调试样例分析约280ms，实际文件导入及渲染约1.9s。CodeMirror大段CDP insertText不等于真实导入耗时，不再以虚拟DOM全文可见性验收。
- 保存期间继续编辑保持新草稿，错误记录数组JSON不崩溃，模板保存立即标旧产物过期；项目ID刷新恢复。
- 审计通过既定门槛，锁文件未变；根Tokio1.53.1仍有RUSTSEC-2026-0386 informational/unsound公告，未升级或忽略。协议/浏览器本机loopback与公告网络需沙箱外执行；原权限失败不计通过。
- 一次历史path_escape偶发，独立与统一重跑通过，保留previous-failure.json；未放宽守卫或宣称根因修复。原FTU git状态读取Permission denied。
- 本机索引 target/engineering-delivery.json，Windows开发包 target/ftu-plugin-0.1.0-windows-x86_64-dev.zip。无提交/推送/发布；Linux/macOS、远端CI、控制器/实机未执行。
