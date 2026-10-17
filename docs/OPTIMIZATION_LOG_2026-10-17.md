# 持续优化记录（2026-10-17）

在保持功能范围不变的前提下修复缺陷、精简代码并收敛架构。工作自基线 HEAD `08ab296` 加既有未提交改动继续；开始前已冻结完整工作树（`target/optimization-20261017/before`，347 文件），可回退。审查与修复分开进行，每条都先确认机制或复现，再最小修复并补回归。

## 一、修复的缺陷

| 位置 | 问题 | 修复 |
| --- | --- | --- |
| `ui/src/App.tsx` | 忙碌期间按 `Ctrl+Enter` 会再建一个任务；弹窗打开时快捷键仍在背后生效 | 生成入口与按钮共用同一忙碌门控；快捷键在存在 dialog/menu 时不触发 |
| `ui/src/App.tsx` | 执行记录的“恢复输入”对受管动作是静默空操作 | 改用 `restoreDocument`，受管记录可恢复并保留冻结参数、扩展选择与运行绑定 |
| `ui/src/App.tsx` | “识别参数”晚响应会用请求发起时的规格覆盖期间编辑 | 按文档标识、源码与规格校验，版本不符即丢弃并提示 |
| `ui/src/App.tsx` | 复制/另存副本继承 `lastRunId`，刷新后副本显示原任务结果且可导出 | 新增 `cloneDocumentForCopy` 清空执行绑定；“加载磁盘版本”改为保留当前运行绑定 |
| `ui/src/components/PluginSettings.tsx` | 检查目录 A 期间改为 B，A 的迟到响应仍被展示，安装却用 B；保存按钮在写入完成前重新可点并覆盖草稿 | 检查按 source 身份丢弃迟到响应；新增 saving 门禁与编辑版本号，指纹仍推进 |
| `ui/src/components/HistoryPage.tsx` | 切换所选记录后旧比较仍显示 | 比较绑定所选记录与请求序号，迟到响应作废 |
| `plugins/template` | `inspect` 与 `save` 的组锁与进程内锁顺序相反，并发时产生伪冲突 | 统一为 `access → group`；渲染全程不持锁，取消与快照语义不变 |
| `plugins/template` | 名为 `__inline__` 的真实模板会抢占纯源码草稿的 schema/defaults/元数据 | root 资产只来自显式 `template`；显式引用的冻结校验与拒绝无法绕过 |
| `plugins/parameters` | `parameter.dependencies.check` 比对未取组锁，与并发导入交叉读到中间态 | 比对与导入同取组锁 |
| `cli/src/workbench.rs` | 孤立或缺失的任务索引会让全部 CLI/UI 启动失败；畸形索引还会 panic | 非法 id 跳过，`not_found` 视为可重建索引并清理，真实写盘失败仍上抛 |
| `runtime/src/config.rs` | 任一无关或损坏的插件目录会让 `boot` 与插件管理整体失败 | 无关目录跳过，损坏清单记录并在被选用时以明确错误 fail-closed |

## 二、精简与架构收敛

- 删除从未被生产入口引用的 `ui/src/features/workbench/**`（11 源 + 6 测试）与 `ui/src/components/ChoiceTabs.tsx`。删除前已核对其 5 条不变量（忙碌门控、识别守卫、运行绑定清理、快捷键作用域、恢复文档）全部移植到活动入口，并确认 18 个文件在冻结快照中留存。
- 移除仅服务该孤立入口的 `zustand`、`sonner`，以及无配置、无脚本、无引用的 `prettier`；同步删除只给 sonner 用的主题变量。
- 移除资产层无消费者的 YAML 数值扫描与确认分支及 `serde_yaml` 依赖，`json_num.rs` 由 1316 行减到 860 行；JSON 的整数边界、下溢候选与确认、字符串/键名跳过、次正规数放行全部保留并有回归。
- App 的 `normalizeSchema`／`prepareDocument` 与副本规则下沉到 `ui/src/lib/documentModel.ts`，活动入口不再各自维护一份。

## 三、工具与依赖

- 定向更新被撤回的 `yoke-derive` 0.8.3 → 0.8.4；YAML 分支移除同时消除 `unsafe-libyaml` 无维护依赖。Rust 与 npm 审计现为 **0 漏洞、0 告警**。
- `build_ui.mjs` 的资源指纹路径统一为 `/`，修复 Windows 反斜杠导致的跨平台校验不稳定，新增独立回归。
- `audit_dependencies.py` 在工具超时时保留已收到的部分 stdout/stderr 作证据，仍判失败；补充对应回归。

## 四、本轮验证结果

平台 Windows 11，基线 `08ab296` 加未提交改动。原始日志在 `target/optimization-20261017/`。

| 检查 | 结果 |
| --- | --- |
| 默认 Rust（`--workspace --all-targets --locked`） | 277 项通过 |
| math Rust（同上 `--all-features`） | 277 项通过 |
| 文档测试 | 通过 |
| fmt / Clippy `-D warnings` / Rust 1.89 | 通过 |
| 前端 lint + 测试 | 无未使用声明；20 文件 141 项通过 |
| Python 工具与归档测试 | 32 项通过 |
| 架构与资源一致性 `check_v2.py` | 通过 |
| 三组隔离浏览器（主流程 / 输入边界 / 共享参数与数学） | 全部通过 |
| 依赖审计 | Rust 0 漏洞 0 告警；npm 0 漏洞 |
| 桌面预览 | 无控制台错误、无失败请求；“工作区资产”与相邻按钮样式一致 |

前端测试文件数由 26 降到 20，是因为删除了 6 个只覆盖孤立实现的测试；活动入口的回归另行补充（`App.executionFlows.test.tsx`、`documentModel.test.ts` 等）。

## 五、体积与首屏测量（同日续）

在同一台机器、release profile（`lto`、`strip`、`codegen-units=1`）与同一工作树下测量。

### 二进制

| 构建 | 体积 |
| --- | --- |
| 默认（template + parameters） | 9,236,480 B（8.81 MiB） |
| `--features math` | 9,295,360 B（8.87 MiB） |

其中约 0.9 MB 是内嵌的 WebUI 资源（`cli/ui`，`include_bytes!` 原样嵌入）。

### 启动

| 场景 | 中位 | 最小 |
| --- | --- | --- |
| `--version`（进程启动） | 7.4 ms | 6.3 ms |
| `actions`（引导插件运行时） | 30.7 ms | 29.0 ms |

### 首屏（gzip）

| 分片 | 优化前 | 优化后 | 加载时机 |
| --- | --- | --- | --- |
| 入口 `index` | 73.7 KB | 63.5 KB | 首屏 |
| `react` | 69.1 KB | 69.1 KB | 首屏 |
| 样式 | 9.0 KB | 8.9 KB | 首屏 |
| `editor`（CodeMirror） | 138.1 KB | 0（移出首屏） | 打开编辑器时 |
| **首屏合计** | **291.0 KB** | **141.3 KB** | — |

首屏请求从 4 个资源减为 3 个，gzip 体积下降 **51.4%**。

### 本次采用的优化

- 编辑器按需加载：新增 `ui/src/components/LazyEditor.tsx`，把 CodeMirror 从入口静态依赖改为 `lazy(() => import(...))`，用同尺寸占位块避免布局跳动。编辑模式、JSON 输入、语法提示之外都不再下载它。
- 次级页面拆分：历史、语法、插件页与通用插件页改为按需加载；历史页同时带走唯一的 `diff` 依赖。
- 验证：首屏网络请求实测只有入口、React 与样式；进入语法页才请求其分片；打开编辑器才请求 `Editor` 包装与 `editor` 分片。三组浏览器与 141 项前端测试仍全部通过。

### 尚未采用的体积手段

- 二进制约 0.9 MB 是原样内嵌的 WebUI 资源；若改为压缩内嵌并在服务端解压，可减小体积，但会新增解压依赖与运行时开销，收益与复杂度需再评估。
- `jsonschema` 0.33 的 `regex`／`fancy-regex`／`regex-syntax` 是无法通过 feature 关闭的必需依赖，是除内嵌资源外最大的可减项；替换为最小校验器会改变 schema 语义，风险高，先行记录而不实施。
- 未采用 `panic = "abort"`：它虽可减小体积，但会让单个请求处理中的 panic 终止整个服务，属健壮性回退。

## 六、边界与未完成

- 未新增功能，未改历史评审与归档事实记录；保留旧加工拒绝、冻结快照、精度、指纹、取消与整体交付语义。
- `docs/PROJECT_REVIEW_REPORT_2026-10-15.md` 使用 `file.ext:NNN` 形式的引用不是有效的 GitHub 行锚，属历史记录格式，未改写其事实。
- 首屏测量基于 `ui/dist/index.html` 实际请求的资源与 gzip 体积；本机结果不代替其它平台或真实网络。
- 未取得最终提交的 Linux/macOS/Windows 远端 CI，未构建发行资产；未提交、推送或发布。
- 后续仍可继续：在 App 内按职责抽取小模块、评估内嵌资源压缩、清理历史文档中的过时引用。
