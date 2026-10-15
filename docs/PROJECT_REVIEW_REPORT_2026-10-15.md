# NCtool 项目审查报告

日期：2026-10-15。平台：Windows 11 x86_64，Node v24.15.0，Python 3.13.14。依据 [审查计划](PROJECT_REVIEW_PLAN_2026-10-15.md) 和项目 nctool-review/nctool-verify 规则。

## 结论

确认 8 项可行动缺陷：4 项 P1、4 项 P2。涉及旧加工模板引用绕过、产物静默覆盖、跨文档撤销、异步保存/识别覆盖草稿，以及依赖和目录状态误判。建议先修复 P1 再准备发行。本轮只审查和隔离复现，未修改产品源码、生成资源或锁文件，未提交、推送、评论或发布。

另记录 1 项已观察到但产品语义需厘清的提交失败行为及 2 项尚未专项复现的疑点，不将其计入确认缺陷。没有确认 P0 问题，不代表不存在未覆盖缺陷。

基准 HEAD：`e8fafd5e47a032d52f9f7a6adfbb384d454e0e15`，实际审查对象为该提交加完整未提交工作树，包括新增文件。开始时保存逐文件 SHA-256；结束时全部原有文件摘要一致，`cli/ui/source.sha256` 原值未变。基线和运行证据在 `target/project-review-20261015/`。该目录被 Git 忽略，不保证随仓库分发。

## 修复进展

2026-10-15：用户授权的四项 P1（R1–R4）与四项 P2（R5–R8）均已完成修复及正式回归，分别见 [P1 修复记录](P1_FIXES_2026-10-15.md)和 [P2 修复记录](P2_FIXES_2026-10-15.md)。下文保留审查时的缺陷证据；各条目中的"已修复"标注仅为索引，完整实现取舍、回归清单与验证矩阵以两份修复记录为准。新增快照格式 2 的旧引用重放限制见 P1 修复记录。

## 确认缺陷

### R1 · P1 · 被引用旧加工模板可以绕过退役校验

位置：[plugins/template/src/lib.rs:139](../plugins/template/src/lib.rs:139)，来源构造见同文件第 51–65 行。

保存资产 `old-nc`，内容为 `{"source":"G0 X{{ x }}","metadata":{"nc":null}}`。直接运行 `template.render`，输入 `{"template":"old-nc","context":{"x":7}}`，正确拒绝并返回 `retired_template`。但普通或受管渲染输入 `{"source":"{% include \"old-nc\" %}","context":{"x":7}}` 均成功，交付 `G0 X7`。

根因是 sources 只保留各模板源码，只有根模板保留 asset；新增 ensure_generic_template 只检查根 asset。被引用资产的 metadata.nc/执行意图已丢失，受管 replay_input 中也只剩源码。用户未显式移除旧加工标记，旧资产却通过普通包装模板降级执行。

建议保留并校验实际引用资产的兼容意图，覆盖普通、受管和快照路径。不要因为库中存在一个未被使用的旧模板就拒绝所有通用渲染。退役守卫是本轮新增，本项属于该约束覆盖不完整。

证据：`target/project-review-20261015/retired-include/evidence.json`，包含直接拒绝、普通 include 成功、受管 include 成功的输入、退出码与完整结果。已有 retirement 测试覆盖根资产，缺少引用资产场景。已在本轮 default 二进制复现。

### R2 · P1 · Windows 等价文件名造成多文件产物静默丢失

位置：[cli/src/main.rs:197](../cli/src/main.rs:197)，写入与交付在第 227–239 行。

外部插件返回两个合法 Artifact：`A.txt=FIRST`、`a.txt=SECOND`。执行 `run delivery-probe.run --input - --out-dir <新目录>`，Windows 上退出码为 0，stdout 仍列出两个产物，实际目录只有 `a.txt=SECOND`。`a.txt` 与 `a.txt.`、`a.txt` 与 `a.txt ` 也复现同一行为。

根因是大小写敏感的 BTreeSet 只按字符串判唯一，随后 write_output 顺序覆盖了 staging 内同一物理文件，最终 rename 仍成功。违反完整多文件交付契约，调用方无法从成功状态发现产物已丢失。

建议按目标文件系统的等价名称拒绝冲突，并让 staging 文件采用拒绝覆盖的创建语义；任何碰撞整批失败。本段代码在 HEAD 已存在，属于完整项目审查发现的既有缺陷。

证据：`target/project-review-20261015/entrypoint-probe/results.json`、`filename-alias-results.json`；复现入口 `entrypoint_probe.py`、`filename_alias_probe.py`。均使用本轮 math 二进制、隔离插件/home/workspace。其他大小写不敏感文件系统可能同类受影响，未在 macOS 实测。

### R3 · P1 · 高级规格跨文档共享撤销历史，可将 A 的 schema 写入 B

位置：[ui/src/components/Editor.tsx:230](../ui/src/components/Editor.tsx:230)，调用链为 [WorkbenchUi.tsx:245](../ui/src/components/WorkbenchUi.tsx:245) 与 [AuthorPanes.tsx:84](../ui/src/features/workbench/AuthorPanes.tsx:84)。

文档 A、B 均停留在高级规格页。A 的 schema 含 alpha，切到含 beta 的 B 后撤销，编辑器恢复 A 的内容，并通过当前 B 的 onChange 写入 B。后续保存可持久化错误规格。

源码编辑器有 editorKey={doc.key}，但 JsonEditor 没有文档身份。外部 value 全量替换进入 CodeMirror 历史；externalValue 只阻止该次同步通知，不阻止后续 undo。父级没有按文档 key 重建该规格编辑器。

建议按文档隔离 JsonEditor/EditorState，并明确外部替换与撤销历史的关系。证据：`target/project-review-20261015/ui-review/editor.test.tsx` 使用真实 JsonEditor/CodeMirror，切换 A/B 后调用 undo，确认 B 回调收到 A 的 schema。jsdom 仅补齐几何测量接口，没有 mock 编辑器。属于组件层已复现，未将其表述为真实浏览器专项已通过。

### R4 · P1 · 共享参数保存晚返回会覆盖新编辑和持久草稿

位置：[ui/src/plugins/ParameterViews.tsx:59](../ui/src/plugins/ParameterViews.tsx:59)，patch 在第 36–39 行写 state 与 localStorage。

提交 title=submitted 的参数资产，在保存请求未返回前继续编辑为 new unsaved edit。响应返回后，patch({...draft, expected:...}) 使用提交时闭包里的旧 draft，界面恢复 submitted，新编辑消失，localStorage 同时被旧草稿覆盖。期间新建、读取或切换资产也可能被旧响应替换。

建议将响应绑定到资产身份与草稿版本，仅更新对应版本的 fingerprint，并保留保存期间的新编辑。不要将“已保存提交版本”解释成可以覆盖后来输入。

证据：`target/project-review-20261015/ui-review/repro.test.tsx` 参数保存测试，延迟 parameter.save 后修改草稿，放行响应确认新编辑消失。该复现 mock API 响应顺序，使用真实 ParameterLibrary 与持久化逻辑。

### R5 · P2 · 原先缺失的可选引用被创建后，依赖检查仍误报 current（已修复）

位置：[plugins/parameters/src/lib.rs:528](../plugins/parameters/src/lib.rs:528)。前端消费在 [managedDependencies.ts:91](../ui/src/lib/managedDependencies.ts:91)。

空工作区受管渲染 `A{% include "optional" ignore missing %}Z`，输出 AZ。随后保存 optional 模板，源码为 NEW。用首次结果的 parameter_snapshot 和 template snapshot 调用 parameter.dependencies.check，仍返回 `{"current":true,"changes":[]}`；但同一实时输入重新生成已输出 ANEWZ。

检查仅遍历旧模板集合的键，无法发现原先缺失、现在可解析的依赖。前端依赖轮询据此将旧结果标为当前并允许导出。历史冻结重跑仍输出 AZ 是正确行为；这里的缺陷是当前结果过期状态判断错误，不是历史保留原产物。

建议双向比较能影响解析的模板集合，或记录缺失可选/动态依赖的状态。实现在 HEAD 已存在，属于既有缺陷。

证据：`target/project-review-20261015/dependencies-new-include/evidence.json`，保存 first/check/current 三个完整 ActionResult。后端已用本轮 default 二进制复现，前端导出影响由实际调用链推导，未对该场景另跑浏览器。

**修复（2026-10-15）**：依赖范围改为由冻结来源集合自身推导。检查先用冻结 snapshot 调用 `template.inspect`，按 include/import/from/extends 得到该结果实际依赖的模板名集合，再与当前磁盘集合逐名双向比较源码，覆盖新增可解析、源码变化与删除三种情况；冻结集合中从未被引用的新增资产不进入比较，不会误报过期。根模板源码仍按草稿排除，其变化只经 `template_metadata` 判定。同一 `evidence.json` 场景现已正确返回 `{"current":false,"changes":["templates/optional"]}`，真实 CLI 复现通过；`cli/tests/managed_parameters.rs` 新增 3 项回归（含不误报无关新增资产、冻结 snapshot 仍重放 AZ）。证据目录 `target/r5-verify/`，被 Git 忽略。未改 `template.render.managed`、R2–R4 以外的 P2 项或 UI。

### R6 · P2 · 参数识别的晚响应覆盖请求期间的新 schema（已修复）

位置：[ui/src/features/workbench/useWorkbenchController.tsx:234](../ui/src/features/workbench/useWorkbenchController.tsx:234)。

对 `{{ old }}` 点击识别参数；请求未返回时将源码改成 `{{ newer }}` 并定义 newer:number。响应返回后 source 仍为 newer，但 schema 中新版字段被删除、只剩 old。discover 从请求前 doc.asset 复制 schema，随后将整份旧规格回写，没有核对源码或 schema 版本。

建议丢弃已过期识别结果，或在确认源码版本匹配后合并到最新规格。editDoc 的闭包文档 key 可避免该请求直接写入其他文档，但无法保护同文档的新输入。

证据：`target/project-review-20261015/ui-review/repro.test.tsx` discover 测试，真实 controller hook 与可控 inspect 响应。新提取的 controller 属于当前新增文件；没有把文件新增本身当作此行为首次出现的证据。

### R7 · P2 · 参数目录跨类型乱序响应，导致加载错误类型草稿（已修复）

位置：[ui/src/plugins/ParameterViews.tsx:41](../ui/src/plugins/ParameterViews.tsx:41)，读取拼接在第 123–129 行。

definitions 目录尚未返回时切到 presets；presets 先完成，definitions 后完成。旧请求把 items 替换成定义列表，但资产类型仍是 presets。选择该项后，预置草稿加载了参数定义的 schema 和 fingerprint。

建议按 group/request identity 丢弃旧目录响应，切组时清空不属于当前组的目录。后端校验及 expected 仍限制部分保存后果；没有证据声称已导致后端跨组覆盖。

证据：`target/project-review-20261015/ui-review/repro.test.tsx` 目录乱序测试，可控目录响应顺序，确认 presets 类型下加载 definition 草稿。

### R8 · P2 · 快捷键绕过运行中门禁，并能在其他页面生成隐藏模板（已修复）

位置：[ui/src/features/workbench/useWorkspaceSession.ts:145](../ui/src/features/workbench/useWorkspaceSession.ts:145)，生成入口见 [useWorkbenchController.tsx:258](../ui/src/features/workbench/useWorkbenchController.tsx:258)。

首个生成请求已创建 queued 任务，此时 busy=true、生成按钮禁用，但 Ctrl+Enter 仍创建第二个任务；切到共享参数页后再按 Ctrl+Enter，又创建第三个隐藏模板任务，页面仍为 parameters。全局快捷键没有页面/模态作用域约束，generate 也只检查提交中的瞬时状态，没有拒绝已有 queued/running。

建议将可执行条件统一放在生成入口，并限制快捷键作用域，保证按钮与快捷键遵循相同门禁。该行为在 HEAD 的旧 App 中也存在，是既有缺陷。证据：独立 repro.test.tsx 的快捷键测试使用真实 controller/session，记录调用计数 1→2→3；没有把重复排队夸大为取消或产物隔离突破。

R3、R4、R6、R7 的对应逻辑也经 HEAD 对照确认为既有行为；本轮重构移动/拆分代码没有消除这些缺陷。

## 单独记录的行为与待验证项

- **生成提交失败后旧成功结果仍可导出：行为已复现，暂不计确认缺陷。** controller 第 287–322 行仅在 POST runs 成功后替换 lastRunId；网络失败/拒绝时只通知，旧成功结果仍是 latest，exportReason 为空。独立提交失败测试已验证。尚需厘清“同签名输入再次提交失败”是否必须使先前结果失效，以及如何展示旧结果来源。不要将其混同为服务端已创建任务进入 failed 后仍可导出；后者有门禁且测试通过。
- 历史比较响应：HistoryPage 的 compare 回包缺少类似 replay-check 的选择身份守卫；需专项乱序响应验证。
- 插件设置保存期间继续编辑：回包后刷新配置可能覆盖新草稿；本轮未专项复现。

## 本轮验证结果

所有命令与退出码见 `target/project-review-20261015/checks.json`。Rust workspace 检查均带 --workspace 和 --locked，默认/math 使用不同 target，固定本轮二进制。核对结果如下：

| 检查 | 实际结果 | 证据 |
| --- | --- | --- |
| 前端 lint（tsc 类型/unused） | 通过 | frontend-types.log |
| 前端现有测试 | 12 文件、62 项通过 | frontend-tests.log |
| 嵌入资源 --check | 通过，未重建资源 | embedded-resources.log |
| 架构/包边界 check_v2 | 通过 | architecture.log |
| Python 工具与归档测试 | 24 项通过 | python-tests.log |
| Rustfmt | 通过 | rust-format.log |
| Rust 默认 workspace all-targets | 248 项通过 | rust-default.log |
| Rust all-features workspace all-targets | 248 项通过 | rust-math.log |
| Clippy all-targets/all-features，-D warnings | 通过 | rust-clippy.log |
| Rust 文档测试 | 2 项通过 | rust-doc.log |
| Rust 1.89 workspace all-features check | 通过 | msrv.log |
| 默认与 math CLI 构建 | 均通过，独立 target | build-default.log、build-math.log |
| 通用浏览器回归 | 通过 | browser-main/report.json |
| 失败/取消/兼容边界浏览器回归 | 通过 | browser-boundaries/report.json |
| 主题/布局/焦点浏览器验收 | 通过，4 种窗口尺寸，errors=[] | browser-layout/report.json 及截图 |
| 独立 UI 疑点复现 | 2 文件、6 项通过；这是缺陷行为复现成功 | ui-review-tests.log |
| 数值最小 CLI 场景 | 大整数原值保留，越界/下溢/非有限输入拒绝，null/负零/最小次正规值检查通过 | entrypoint-probe/results.json |
| math 三条件与受管冻结 | 符合预期 | math-boundary-probe/results.json |
| 当前入口文档本地目标链接 | 无缺失文件目标；不含完整历史归档/全锚点校验 | docs-links.json |
| git diff --check | 通过，仅行尾规范化提示 | 本轮命令输出 |
| npm 依赖审计 | 完成，0 漏洞 | audit/ui.json |
| Rust 依赖审计 | **未完成**，RustSec 数据库获取 300 秒超时；审计脚本退出 2 | audit/summary.json、audit/cargo.stderr.txt |
| 源码/锁文件/嵌入来源指纹 | 原有文件全部未变、锁文件未变 | baseline.json、provenance.json、audit/summary.json |

审计失败命令：`python scripts/audit_dependencies.py --output target/project-review-20261015/audit`。其 cargo audit 子进程下载公告数据库超时；没有用历史零漏洞结果替代，也没有自动升级依赖。

浏览器使用既有独立 Playwright 安装与可用 Chromium 1208，NCTOOL_BINARY 固定为本轮 math/debug/nctool.exe，三组脚本各自使用临时 home/workspace 和空闲 loopback 端口。未触碰真实用户工作台，未关闭用户进程。本轮没有因只读审查启动额外桌面预览宿主；具体疑点的 UI 证据是组件测试，不冒称浏览器复现。

## 覆盖与限制

- 已沿配置迁移、保留名拦截、模板/受管渲染、历史环境/来源、资产与集合、任务/RPC、CLI/HTTP、math、WebUI 状态及 CI/归档链路审阅。除所列问题外，没有确认其他可信正确性缺陷。
- 旧配置预览/显式 expected 迁移、原字节备份、混合退役集合整体拒绝、外部同名接管拒绝、历史原 artifact 查看及环境检查均核对了实现和本轮集成测试。没有将有意退役当作功能缺失。
- HTTP Host/Origin/Content-Type/体积限制及 RPC 崩溃、堵塞 stdin、超时、取消、后续恢复已有本轮测试与脚本证据。没有进行压力/模糊测试，或完成/取消极窄竞态压力注入。
- 未注入断电/磁盘故障/跨 provider 回滚失败；不能据普通回滚测试证明所有故障下的跨集合原子性。
- 全部运行结果仅为本轮 Windows 本机证据；未查询或运行拟发布提交的远端 Linux/Windows/macOS CI，未生成发行资产。CI/release 仅静态审阅。
- 归档测试证明保存字节与清单一致，未解压运行旧软件。机床工艺验收及仓库外产品不在范围内。
- 编辑器跨文档撤销已组件复现；完整浏览器选区、撤销历史和诊断跳转未专项验收。现有主题验收通过不能推出这些行为正确。
- 文档 RELEASE.md 第 23 行仍提到已移出的轴类门槛，部分现行说明仍写“两组”浏览器；建议后续文档同步时按实际三组 CI 修正。作为文档维护项，不计入上述正确性缺陷。

## 建议修复顺序

R1–R8 均已修复并补回归（顺序见两份修复记录）。若准备发行，需补齐联网 Rust 审计与目标提交的实际跨平台 CI；本机结果不能替代远端三平台。审查报告本身不授权修复或发布。
