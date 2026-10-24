# 文档与记忆对齐记录（2026-10-24）

本轮仅整理文档与记忆，未修改业务源码、依赖、生成资源或用户运行资产。基准为本地分支 `codex/release-v2.3.0-record`、HEAD `cba817c3dcef7f9106466de15bd710b94be2a3b0`；原有未跟踪 `.codex/`、`memory/` 和插件提案保留。不自动提交、推送、合并或发布。

## 当前事实摘要

| 项目 | 当前事实 | 依据 |
| --- | --- | --- |
| 版本 | CLI/WebUI 2.3.0；引擎、SDK、运行时及内置插件 2.0.0；外部协议 2，服务版本独立 | `cli/Cargo.toml`、`ui/package.json`、workspace 组件清单、`sdk/src/lib.rs::PROTOCOL_VERSION` |
| 默认能力 | template + parameters；共享定义、预置、绑定、来源、冻结参数及受管渲染仍存在 | 根 workspace、CLI composition、`plugins/parameters/src/lib.rs`；[参数说明](PARAMETER_MANAGEMENT.md) |
| 数学 | math 是唯一可选 feature，需编译、运行配置启用、单次 extensions 选择 | CLI features、composition 与[模板指南](TEMPLATE_WRITING_GUIDE.md) |
| WebUI | Ant Design、Hash 路由、TanStack Query、Zustand；CodeMirror 按需加载 | `ui/package.json`、`ui/src/App.tsx` 和[重写说明](UI_REWRITE.md) |
| 外部插件 | 协议 2、Schema 表单、文本产物、任务/历史、取消/超时；同用户权限运行 | SDK 的 ActionResult/Artifact、通用动作页和[协议](PLUGIN_PROTOCOL.md) |
| 加工边界 | 无内置 NC/机床/工序/轨迹实现，不恢复旧加工动作；领域实现放独立外部插件 | 当前 workspace、项目规则、[架构](ARCHITECTURE.md) |
| 后续提案 | 六类加工工具插件，首批建议切削与坐标计算；顺序和功能仍待确认，尚未实施 | [插件提案](MACHINING_PLUGINS_PLAN_2026-10-24.md)、[路线图](ROADMAP.md) |

摘要是本次核验的快照；后续实现以源码、清单和实际运行证据为准。

## 远端复核与证据范围

2026-10-24 通过已连接的 GitHub 工具只读核验实际仓库 `lzg9698-code/nctool-tpl`：

- [v2.3.0 正式发行](https://github.com/lzg9698-code/nctool-tpl/releases/tag/v2.3.0) 非草稿、非预发布，latest 接口也返回该版本。带注释标签对象 `8dad84572e7c1dcd7db95430dd487d93a66aeaff` 解析到提交 `ad0fbd85e5a1e431734f2846d9fda9cd4ba289da`。
- [发布运行 40043531922](https://github.com/lzg9698-code/nctool-tpl/actions/runs/40043531922) 对应上述提交，状态 completed/success。jobs 接口共返回 9 项：三平台验证、MSRV、依赖审计、三平台构建及发布，全部 success，没有后续分页。
- Release 有 Windows/Linux/macOS 三个二进制及同名 SHA-256 文件，共 6 个资产。本轮将 API 中三个二进制 digest 与本机既有 `target/release-v2.3.0/checksums.json` 逐项比较，并重新计算本机已有三个下载文件的 SHA-256，均一致；没有重新下载或运行发行程序。此前实际下载、Windows CLI/30 组浏览器及 Linux/macOS 未在本机执行的边界仍见[发行签收](PROJECT_STATUS.md#v230-正式发行2026-10-24)。
- [PR #40](https://github.com/lzg9698-code/nctool-tpl/pull/40) 已合并到发行提交。[签收文档 PR #42](https://github.com/lzg9698-code/nctool-tpl/pull/42) 已于 2026-10-24 合并，合并提交为 `2502958ceb9dc5df19ef3103ca65edb0361514d7`，其 head 为本地基准 `cba817c3dcef7f9106466de15bd710b94be2a3b0`。本轮未切换或更新本地分支。

远端成功只证明对应提交。本轮文档与记忆新增内容未提交、未运行远端 CI；本轮也不重新签收产品测试、性能或控制器验证。

## 已处理的差异

| 原表述或问题 | 对齐方式 |
| --- | --- |
| 当前架构、契约、协议、系统设计和 WebUI 指南仍写“2.3.0 未发布” | 更新适用版本与发行证据入口，不改变 API 契约 |
| 参数、模板指南仍写“当前 2.2 工作台” | 更新为 2.3.0，并明确共享参数恢复后的现状 |
| 参数指南仍称受管上下文不支持对象/null | 按解析与模板调用链修正为支持嵌套对象/null，数值边界与统一 JSON 入口一致 |
| 路线图仍把发行所需三平台 CI 列为未完成 | 明确 v2.3.0 已完成，只要求后续改动重新取得相应验证 |
| 旧预览方案仍可被读作当前参数方案 | 增加历史提示，保留原文及当时验证，不替换旧数字 |
| 旧审计自称“唯一进度载体”，含并行分支与待合并指令 | 增加历史范围说明，不把旧审计条目推断为当前全部通过 |
| 旧会话收尾含预览进程、待查 CI 和阶段待办 | 增加历史范围说明，不操作旧进程，不把旧建议直接转为当前任务 |
| 文档导航没有插件提案，路线图说明仍称 P0–P3 | 补充提案和对齐记录入口，按实际路线图描述导航 |
| 状态页混有阶段记录和当前结论 | 当前基线、能力、限制在前；保留各轮及发行的独立证据范围 |

既有开发边界快照和旧 UI 选型已有明确历史提示，本轮不重写其正文。CHANGELOG 原有历史及归档文件保持不变。

## 记忆与规则维护边界

- 仓库会话摘要新增到 `memory/inbox/`，不覆盖既有笔记。
- 中心项目简报 `D:/ai-brain/memory/projects/nctool-rustjinja/_brief.md` 仍写 2.2 和旧发布待办，并引用已归档的原路径。修订通过中心 `memory/inbox/` 新增纠偏记录表达，不直接覆盖简报；旧路径的当前归档位置可由[历史索引](archive/README.md)查询。
- Codex MEMORY.md 的模板自管参数结论及旧发行观察只适用于原历史分支。纠偏请求仅新增到 `C:/Users/Admin/.codex/memories/extensions/ad_hoc/notes/`；不直接改 MEMORY.md、摘要或 rollout。新增请求不等于记忆已被系统吸收。
- 仓库 AGENTS.md 与部分专用技能仍有 CLI/WebUI 2.2 的旧口径。本轮登记该差异，保留规则和技能文件原状；版本事实依清单解释，不修改权威来源未知的副本。
- 中心 inbox 与 Codex 更新请求只记录关键纠偏并指向本摘要，不重复整套文档。本轮已分别新增 `D:/ai-brain/memory/inbox/2026-10-24-084516-nctool-brief-correction.md` 和 `C:/Users/Admin/.codex/memories/extensions/ad_hoc/notes/2026-10-24-084516-nctool-current-state.md`，复制摘要与元数据检查通过；中心简报与聚合记忆原文仍未直接更新。

## 本轮验证

按当前贡献指南的纯文档范围，检查活动文档相对链接和锚点、涉及接口与命令的源码依据、JSON 示例、历史正文保留、记忆 frontmatter、空白差异及修改范围。新插件只检查提案，不执行业务验收。

实际检查：20 份活动文档的 164 个本地链接与章节锚点、11 个 JSON 示例均通过；三份新增历史提示移除后与 HEAD 原文逐字一致；三条新增记忆的 by/date 元数据通过；本机已有三个发行二进制摘要与 API 及旧签收记录一致。`git diff --check` 通过，受版本控制的改动仅涉及文档。

本轮未改任何命令或 JSON 业务示例；不重新构建 UI 或二进制、不跑无关全量测试。远端复核摘要和本地检查产物保存在忽略目录 `target/doc-memory-alignment-20261024/`，不保证随仓库提供。检查使用桌面内置 Python 的绝对路径；系统 `python` 指向不可执行的 WindowsApps 别名，首次调用失败，改用内置解释器后检查通过。
