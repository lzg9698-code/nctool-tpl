# NCtool 2.2 阶段收尾代码审查

审查日期：2026-10-02。审查对象是当前完整工作树，包含已暂存、未暂存及新增源码；不是仅审查 HEAD。HEAD 为 `d06d7e314f12d39aac368c04858fbf1ed0a707d4`，2.0–2.2 的主要升级仍在本地改动中。

**2026-10-02 审查结论（历史）：确认 10 项缺陷，7 项 P1、3 项 P2，彼时未达到收尾条件。2026-10-03 已修复 R01–R10；关闭证据见文末。** 当前通过的测试不能覆盖下述绕过路径。本次没有修改业务实现、提交代码或改变用户工作区资产；新增本报告及状态入口，复现使用临时 home/workspace。

P1 表示应在本阶段收尾前修复的校验绕过、错误产物或错误输入执行；P2 表示明确的兼容性、精度或界面状态问题。这里的 NC 风险判断来自软件行为，不构成机床工艺验证。

## 一、确认的问题

| 编号 | 优先级 | 问题 | 复现方式 |
| --- | --- | --- | --- |
| R01 | P1 | 非有限值检查漏掉紧凑字址，交付 `XinfY0` | CLI |
| R02 | P1 | 直接传入机床绕过前缀校验，额外指令进入正文 | CLI + HTTP 对照 |
| R03 | P1 | NC 插件未启用时，NC 模板自动回退普通渲染 | Chromium |
| R04 | P1 | 模板工作台的 NC 动作丢失 Schema 范围约束 | HTTP + Chromium |
| R05 | P1 | 动态 include 中的参数规格不参与值校验 | CLI |
| R06 | P1 | 宽松 NC 渲染静默清空缺失的机床字段 | CLI |
| R07 | P1 | 通用插件页 JSON 无效时，仍执行上次有效输入 | Chromium |
| R08 | P2 | `ignore missing` 模板在 CLI 成功、后台任务失败 | CLI + HTTP |
| R09 | P2 | 超出 u64 的整数在 JSON 入口静默转换为浮点 | CLI |
| R10 | P2 | 通用插件页修改输入后，旧结果仍可直接导出 | Chromium |

### R01：非有限值检查应按 NC 字址识别，不能依赖空白分词

位置：[plugins/nc/src/lib.rs](../plugins/nc/src/lib.rs)，159–165 行。

输入 `nc.generate`：

```json
{"source":"G1 X{{ 1e308 * 1e308 }}Y0"}
```

实际成功返回 `G1 XinfY0\n`，同时包含可交付 artifact，诊断为空。模板运算能产生非有限值，即使所有输入数字有限；现有检查只识别独立 token 或单字址后紧接完整 `inf`，无法识别后面连接下一字址的情况。

修复：对实际 NC 数值字址进行检查，区分正文、字符串与注释；保留数学/格式化过滤器的有限性保护。不要用全字符串 `contains("inf")` 替代，以免把注释和名称误判。

验收：`Xinf`、`XinfY0`、`X-infZ1`、NaN 及其紧凑形式均拒绝；CLI 非零退出且不写 `--out`，HTTP/后台任务无 artifact，多工序无整体产物；普通注释中的合法文字不误报。

### R02：生成阶段必须复用机床校验

位置：[plugins/nc/src/lib.rs](../plugins/nc/src/lib.rs)，32–38 行；`validate_machine` 位于 217 行附近。

`machine.save` 和机床集合导入调用 `validate_machine`，但 `nc.generate` 的 `machine` 与 `machine_id` 路径只反序列化。以下直接配置成功生成额外指令：

```json
{
  "source":"G0 X1",
  "machine":{"id":"test","vendor":"Example","model":"Virtual",
    "config":{"line_number_prefix":"N\nG0 Z-100\nN"}},
  "options":{"line_numbers":true}
}
```

实际正文为 `N\nG0 Z-100\nN0010 G0 X1\n`。同一配置经 `machine.save` 被 `invalid_machine` 拒绝。这是生成入口与存储入口的校验不一致；历史重执行也会直接传入机床快照。

修复：提取不依赖存储名称的机床内容校验，保存、导入、读入后生成、直接输入及 replay 共用；保存时另检查 ID 与资产名一致。

验收：所有入口拒绝控制字符和非法前缀；合法自定义前缀与历史快照继续可用，失败时无生成产物。

### R03：已标记领域的模板不能因插件缺失而自动改走普通文本

位置：[ui/src/lib/documentExecution.ts](../ui/src/lib/documentExecution.ts)，21–33 行。

新导入/打开的资产仅有 `metadata.nc`、没有显式 `generationAction` 或 `metadata.execution.action` 时，纯模板组合中的 `choices` 不含 NC 动作。领域匹配结果为空后，代码直接选取默认 `template.render`。

Chromium 复现：NC 资产声明 `x >= 0`，源码 `G1 X{{ x }}`、参数 `x=-10`；在 template 组合打开，生成方式自动显示普通文本，后台任务以 `template.render` 成功交付 `G1 X-10`。只有模板使用 `nc_fixed` 等插件过滤器时，才会碰巧因未注册过滤器失败。

修复：领域执行意图需要有稳定、可持久化的动作/能力声明，缺失能力时阻止执行；用户明确选择普通文本才能改变执行方式。避免在通用宿主中堆叠 NC 专用判断。

验收：新打开、导入、草稿恢复与停用插件后的 NC 资产均提示缺失能力；无显式切换就不提交普通渲染任务；普通模板照常使用默认渲染。

### R04：工作台 NC 生成未保留通用参数定义的约束

位置：[plugins/nc/src/lib.rs](../plugins/nc/src/lib.rs)，377 行的文档输入绑定；[ui/src/lib/documentExecution.ts](../ui/src/lib/documentExecution.ts)，35–62 行；[ui/src/lib/data.ts](../ui/src/lib/data.ts)，166–204 行。

NC 文档输入绑定传 `metadata.nc.specs`，不传通用 Schema。工作台总是传当前源码草稿，模板服务因此不再使用磁盘根资产的 Schema。前端 `fieldErrors` 只做部分必选/数值语法检查，没有范围校验。资产只有 Schema、没有 NC specs 时，范围约束完全消失。

Chromium 与 HTTP 均复现：资产 `schema.properties.x.minimum=0`、`metadata.nc={}`，源码 `G1 X{{ x | nc_fixed(3) }}`；工作台输入 `x=-10`，实际 `nc.generate` 成功交付 `G1 X-10.000\n`。相同 Schema 经普通渲染服务会拒绝该输入。专用 NC 页面另外使用 `specsFromSchema`，还造成不同页面语义不一致。

修复：由后端明确处理通用 Schema 与 NC specs 的关系，在冻结源码/元数据后对同一份有效参数同时实施适用约束；显式 specs 不应意外解除 Schema 约束。UI 只提供提示，不能承担唯一校验。

验收：模板工作台、NC 专页、工序、CLI、HTTP 与历史重执行对相同定义作出一致判断；覆盖 minimum/maximum、enum、required、对象/数组及草稿中的参数定义修改。

### R05：动态 include 可以绕过已声明的参数范围

位置：[plugins/template/src/lib.rs](../plugins/template/src/lib.rs)，117–126 行；[plugins/nc/src/validate.rs](../plugins/nc/src/validate.rs)，729 行附近的 `for var in vars`。

静态检查不展开动态模板名；值校验只遍历静态提取到的变量。完整源码集合虽进入快照，实际被动态引用的子模板变量仍不参与规格校验。

```json
{
  "source":"{% include child %}",
  "templates":{"sub":"G1 X{{ x | nc_fixed(3) }}"},
  "params":{"child":"sub","x":-10},
  "specs":[{"name":"x","kind":"number","min":0}]
}
```

实际成功交付 `G1 X-10.000\n`，仅把 x 的规格标为 `spec_inert`、参数标为 `unused`。不能把静态分析未发现等同于实际执行未使用。

修复：对已提供且有规格的参数实施独立值级校验；NC 路径在无法证明动态引用校验完整时应明确拒绝，或针对冻结输入解析实际依赖。通用模板可保留动态引用能力。

验收：上述用例硬失败且无 artifact；补充动态 include/import/extends、子模板多层引用及正常静态模板的兼容用例。

### R06：宽松渲染不能清空未配置的机床字段后继续交付

位置：[plugins/nc/src/lib.rs](../plugins/nc/src/lib.rs)，125–130 行；[plugins/nc/src/validate.rs](../plugins/nc/src/validate.rs)，893–894 行。

```json
{"source":"G1 X{{ machine.zero_x }}","lenient":true}
```

默认机床没有 zero_x，实际成功交付 `G1 X\n`，诊断为空。校验只看到顶层 machine，并把它作为系统变量跳过，未检查实际成员；宽松模板渲染随后将缺失坐标清空。

修复：校验所需机床成员或对 NC 输出采用明确的严格策略。已有 `extract_member_accesses` 可用于静态成员检查；动态下标、条件引用、default 等情形需明确支持边界。

验收：严格与宽松模式均阻止必需机床成员缺失；显式可选字段和合法默认值正常工作；嵌套引用/动态下标不能静默产生不完整 NC。

### R07：通用插件页在 JSON 无效时仍提交旧输入

位置：[ui/src/App.tsx](../ui/src/App.tsx)，2321–2337 行；[ui/src/components/WorkbenchUi.tsx](../ui/src/components/WorkbenchUi.tsx)，144–156 行。

`JsonEditor` 解析失败时保留上一份 value，并只通过可选 `onError` 通知父级。通用插件页没有接这个回调，执行按钮继续提交 value。

Chromium 复现：Python 报告页将高级输入改成 `{`，页面显示错误；点击执行仍创建成功任务，实际输入是旧的 `values=[10,20,30]` 和 `Monthly report`。外部插件也可能执行资产修改或其他副作用，不能以旧值代替当前无效草稿。

修复：父级保存原始 JSON 草稿及有效性，解析失败时阻止执行；表单/JSON 切换和跨页恢复使用同一份状态。清除错误时也应同步验证状态。

验收：无效 JSON、错误顶层类型、未完成数值均不发送 runs 请求；修正后仅提交当前可见输入，切页后无效草稿保留。

### R08：保存快照不应改变 `ignore missing` 的渲染语义

位置：[plugins/template/src/lib.rs](../plugins/template/src/lib.rs)，113–126 行；[cli/src/workbench.rs](../cli/src/workbench.rs)，224–227 行。

源码 `A{% include "absent" ignore missing %}B` 经 CLI `template.render` 成功输出 `AB`；经后台 runs 接口失败，错误 `template_not_found: absent`。工作台为了冻结快照先调用 inspect，inspect 将静态引用全部视作必需文件。

修复：快照冻结与依赖诊断分开；保存实际存在的源码，按 Jinja 的可选引用语义检查。不能为收集证据而把合法渲染变成失败。

验收：CLI、直接动作和后台任务结果一致；覆盖可选 include、候选模板列表、必需引用缺失，以及历史重执行快照。

### R09：JSON 入口对超范围整数应精确保留或明确拒绝

位置：[assets/src/lib.rs](../assets/src/lib.rs)，646–659 行；[assets/Cargo.toml](../assets/Cargo.toml) 的 serde_json 配置。

```json
{"source":"{{ x }}","context":{"x":18446744073709551617}}
```

实际成功渲染 `18446744073709552000.0`。前端保留原始数值，但后端 serde_json 默认表示无法容纳该整数，转为浮点；现有入口仅检测非零数下溢，没有检查整数精度丢失。

修复：定义各层支持的数值范围；超出支持范围时返回结构化错误，或端到端采用精确表示，不能只在浏览器无损。开启 arbitrary_precision 时还需核对 Schema、MiniJinja、NC i64 模型及外部协议，不可仅更改一个 feature。

验收：i64/u64 边界和相邻值、超范围整数及嵌套数据要么原样保存/渲染，要么明确拒绝；资产保存、bundle、run 记录与 replay 都不得静默舍入。

### R10：通用插件结果也要跟踪过期状态

位置：[ui/src/App.tsx](../ui/src/App.tsx)，2250 行附近的状态及 2351 行。

模板和 NC 页面保存任务提交时的输入签名，通用插件页只保存 runId，给 `RunDetail` 的 stale 默认为 false。Chromium 复现：报告生成成功后，把输入改为 `values=[999]`，旧结果仍无过期提示，导出按钮保持可用；切换动作也保留旧 runId。

修复：通用视图记录动作 ID 与输入签名，复用工作台和 NC 页面相同的结果过期规则。历史页仍可明确导出原始历史文件。

验收：编辑表单、JSON 或切换动作后显示旧结果状态并禁用当前操作区导出；重新执行成功后恢复；失败或取消不恢复旧结果为当前产物。

## 二、审查时检查与证据（2026-10-02）

| 检查 | 当前结果 | 本机记录 |
| --- | --- | --- |
| workspace / all-targets / all-features Rust 测试 | 429 项通过 | `target/review-rust-tests.log` |
| Rust 文档测试 | 4 项通过 | `target/review-doc-tests.log` |
| 纯模板 CLI 测试 | 15 项通过 | `target/review-template-only.log` |
| 前端 Vitest | 14 项通过 | 本次终端输出 |
| TypeScript + Vite 生产构建 | 通过 | `target/review-ui-build.log` |
| Clippy，`-D warnings` | 通过 | `target/review-clippy.log` |
| rustfmt | 通过 | `target/review-format.log` |
| Rust 1.89 MSRV | 通过 | `target/review-msrv.log` |
| 架构/纯模板依赖/打包边界检查 | 通过 | `python3 scripts/check_v2.py` 输出 |
| 源码与嵌入前端资源一致性 | 通过 | `target/review-ui-parity.log` |
| 既有 Chromium 完整验收 | 通过 | `target/review-browser.log` |
| npm audit | 当前报告 0 项漏洞 | `target/review-npm-audit.json` |
| `git diff --check` | 通过；已有 bat 文件有 CRLF 提示 | `target/review-diff-check.log` |

定向复现证据：

- `target/review-probes.json`：R01、R02、R06、R09 及直接渲染的 R08 对照。
- `target/review-http-probes.json`：R02 的保存拒绝对照、R04、R08 后台任务失败。
- `target/review-dynamic-include.json`：R05。
- `target/review-browser-probes.cjs`、`.json`、`.log`：R03、R04、R07、R10；截图 `target/review-plugin-input.png`、`target/review-schema-nc.png`。

这些属于本机忽略目录中的验证产物，报告保留关键输入/结果以便其他环境重新复现。npm 审计不等于 Rust 依赖漏洞审计，本轮没有执行 cargo-audit。未运行 Windows/macOS、真实控制器或真实加工验收。

审查覆盖：独立引擎与提取接口、JsonStore/原子写/路径/指纹/锁、SDK 与依赖解析、外部双向 RPC、template/math/NC/process 插件、composition/HTTP/后台任务、React 表单/草稿/历史/插件贡献以及 CI/release。

依赖拆分、显式服务调用、通用渲染隔离、指纹保护、快照与多工序整体交付已有实现和测试，适合作为后续扩展的基础。本次没有据此推断第三方插件任意副作用可回滚，也没有将通过软件测试等同于加工安全。

## 三、收尾实施顺序

| 阶段 | 具体工作与文件 | 交付与验收 |
| --- | --- | --- |
| S1：统一后端输入/产物约束 | R01/R02/R04/R05/R06/R09；`plugins/nc/src/lib.rs`、`validate.rs`、`plugins/template/src/lib.rs`、`assets/src/lib.rs` | 修复及行为回归进入 `cli/tests/platform.rs`、对应 crate 测试；同一输入经 CLI/动作/runs/process/replay 判断一致；失败无产物 |
| S2：统一前端执行状态 | R03/R07/R10；`documentExecution.ts`、`App.tsx`、`WorkbenchUi.tsx` | 能力缺失、原始 JSON、输入签名统一处理；前端单测与 Chromium 对应失败路径通过 |
| S3：模板兼容性与可重复执行 | R08；`plugins/template/src/lib.rs`、`cli/src/workbench.rs` | 快照不改变可选引用语义；固定源码/规格/机床/选项后重执行稳定；支持边界写入协议文档 |
| S4：冻结阶段基线 | `docs/PROJECT_STATUS.md`、`PLUGIN_PROTOCOL.md`、`CONTRIBUTING.md` 与仓库状态 | 10 项逐一关联修复/测试；完整检查与目标平台 CI 通过；用户已有本地改动全部纳入可追溯的提交/归档，之后建立下一阶段基线 |

S1 的边界确定后再完成 S2，避免前端另行复制后端领域规则。可先从 R01 与 R02 的最小失败回归开始，再处理 R04/R05 的规格契约。

新增功能前应补齐三份稳定契约：

1. **执行契约**：声明输入、输出和服务版本；冻结实际执行输入；定义取消、失败、产物过期、replay 和副作用边界。把内置与通用插件页共享的执行状态逻辑集中到可测试的模块。
2. **模板/领域参数契约**：明确通用 Schema、NC specs、默认值、派生值、用户覆盖的优先级和校验范围；明确静态/动态引用支持及数值范围。约束不得随入口或页面变化。
3. **资产契约**：保存/导入/直接生成/历史快照共用内容校验；保留指纹冲突与回滚凭证。后续插件增加集合时，补充预检查无副作用、跨集合失败回滚、冲突拒绝及重复导入用例。

不要在关闭这些缺陷前进行大范围模块重写。`App.tsx` 已超过 2300 行，可在 S2 按文档执行、插件执行、历史/迁移职责拆分，并用上述行为回归保护；不需要改变 Rust 核心＋WebUI 的交付方向。

## 四、阶段签收条件

- R01–R10 均关闭，复现输入变为预期行为并进入持续回归；当前 429 项测试通过只是基线，不能代替新增回归。
- 所有失败/取消路径明确无当前可交付产物；旧产物在当前执行区域有过期标记，历史导出另有明确入口。
- 纯模板组合不载入领域实现，也不自动接受领域模板降级执行。
- CLI、直接 HTTP 动作、后台任务、顺序工序和 replay 对同一规格/输入的校验一致。
- Linux 本机检查与 Windows/macOS CI 分别有证据；真实机床验收单独记录，不用 CI 替代。
- 未提交升级改动和新增文件形成可恢复、可追溯的版本基线；本报告与项目状态不再把未修复事项描述为已收尾。

2026-10-02 当时状态为“审查完成，修复与阶段签收待完成”。以下为后续修复记录。


## 五、修复关闭记录（2026-10-03）

**R01–R10 的软件缺陷均已关闭，并纳入持续回归。** 实现保留动态模板引用、普通模板宽松模式、显式文本选择与历史导出；没有以移除功能代替修复。跨平台发布与真实机床验收仍是独立签收事项，本轮没有声称已经执行。

| 编号 | 实际修复 | 可复核证据 |
| --- | --- | --- |
| R01 | 新增 NC 正文扫描，在后处理完成后拦截紧凑字址中的 Inf/NaN；跳过注释和引号内容 | `plugins/nc/src/output_validation.rs` 单测；`review_r01_r02_r05_r06_failures_never_deliver_artifacts_in_any_execution_surface` |
| R02 | 机床内容校验由保存、导入、直接生成、磁盘读入与 replay 共用 | `review_r02_disk_and_history_machine_configs_share_content_validation`；CLI/动作/runs/process 失败回归 |
| R03 | capabilities 提供已知文档动作关联；缺失能力阻止默认降级；保存动作意图 | `review_r03_disabled_domain_intent_is_described_without_loading_domain_code`；`documentExecution.test.ts`；浏览器缺失能力与显式选择保存 |
| R04 | 源码草稿保留根资产约束；Schema 与 specs 在同一份有效参数上校验，系统机床在 Schema 之后注入；replay 保存显式定义 | `review_r04_schema_specs_defaults_and_replay_are_enforced_on_effective_parameters`；浏览器负坐标拒绝/正坐标生成/replay |
| R05 | 所有提供/派生/default 参数均检查 specs，包含静态分析未看到的参数；不禁用动态引用 | 动态 include/import/extends、多层子模板正负值回归 |
| R06 | NC 实际渲染始终严格，显式 default/is defined 仍可用，普通模板保持独立策略 | 直接成员/动态下标缺失拒绝，可选字段和默认值成功回归 |
| R07 | 通用插件页保存原始 JSON 草稿和解析状态，提交时重新读当前原始输入；受控编辑器同步不触发用户输入回调 | `execution.test.ts`；浏览器无效 JSON/错误顶层类型不提交、跨页和刷新恢复 |
| R08 | inspect 冻结现有源码；引用是否必需由 Jinja 实际控制流决定 | `review_r08_optional_and_required_references_keep_cli_task_and_replay_semantics` |
| R09 | JSON 入口拒绝超范围整数，返回 numeric_range；复用扫描器并保持字符列号，避免重复前缀扫描 | `review_r09_json_integer_boundaries_are_exact_or_rejected_across_ingress`；浏览器真实 HTTP 入口检查 |
| R10 | 通用插件页按动作/提交输入签名判定过期，切换动作清除当前结果；失败和取消无产物 | `execution.test.ts`；浏览器编辑/切换动作/失败/取消/重新成功后的导出状态 |

当前检查：436 项 Rust 单元/集成测试、4 项文档测试、18 项纯模板 CLI 测试、17 项前端测试通过。Clippy（拒绝警告）、Rustfmt、Rust 1.89、无警告 Rust 文档、前端生产构建、架构/打包边界与嵌入资源一致性均通过。既有 Chromium 完整验收和新增 `scripts/browser_review_fixes.cjs` 均通过；新增脚本已加入 Linux 浏览器 CI。

本机证据保存在 `target/fixes-tests.log`、`fixes-doc-tests.log`、`fixes-template-only.log`、`fixes-ui-tests.log`、`fixes-clippy.log`、`fixes-msrv.log`、`fixes-doc.log`、`fixes-ui-build.log`、`fixes-architecture.log`、`fixes-ui-parity.log`、`fixes-browser-existing.log`、`fixes-browser-regressions.log`。截图位于 `target/fixes-browser/`。协议补充及运行命令已写入 PLUGIN_PROTOCOL 和 CONTRIBUTING。

修复前完整源码、Git 状态、暂存与未暂存补丁保存在 `.nctool-backups/2026-10-03-review-fixes/`。修复前源码包 SHA-256 为 `61968cb2cbcdb132e5f93705d487aa47e9cbb076b5d711f50254904ec90eef7f`；修复后的源码归档与校验和保存在同目录 `fixed-sources.tar.gz`、`SHA256SUMS`。未执行 Git 提交、推送或发行，也未更新用户已有工作区资产。

为后续功能保留的边界：独立引擎仍仅依赖 MiniJinja；通用 JSON Schema 校验收敛到 SDK；宿主不导入 NC 模型；Schema/NC 参数类型、默认值优先级、严格字段、取消/过期/replay 和资产校验约定已明确。Windows/macOS CI 及实际控制器验证没有在本轮本机执行，正式跨平台发行仍需它们的证据。
