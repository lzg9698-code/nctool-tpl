# nctool CLI + UI 开发计划

> 版本：草案 v0.1 · 2026-08-31
> 范围：在现有 `nctool-tpl`（模板解析）+ `nctool-core`（生成管线）之上，新增**命令行工具**与**本地 Web UI** 两个交付面。

---

## 1. 现状与目标

### 现状（已核验）

- workspace 三个 crate：`nctool-tpl` v0.3.1（parse / extract_variables / extract_undeclared / Renderer + NC 数值过滤器 + 严格/宽松模式）、`nctool-core` v0.2.0（数据模型 / 参数校验 / 模板注册表 + 5 个内置模板 / 机床预设 Generic·WFL M65·INDEX MS40 / G-code 生成管线）、`nctool-cli` v0.2.0（binary `nctool`）。
- 质量基座齐备：211 项测试、CI（fmt / clippy / test / doc / cargo audit）、criterion benchmark、CHANGELOG。
- **CLI 已完成阶段 0/1**：命令树（templates/inspect/validate/render/machine/config/completion）+ 参数输入（`--param` 类型推断 / `--params-file`）+ 配置层叠 + `--format text|json` 统一输出 + `--lenient` 宽松渲染，golden 测试与库管线逐字节一致。
- **Web UI 尚未开始**：`ui` 子命令已占位（返回"规划于阶段 2"），无服务端、无前端。

### 目标

让模板引擎进入日常工艺工作流，而不是只作为库被示例调用：

1. **CLI `nctool`**：脚本化、可批量、可进 CI，供工艺工程师与自动化流水线使用。
2. **Web UI**：本地运行的可视化参数表单 + 实时 G-code 预览，降低填参与校验门槛。

---

## 2. 总体架构

```
┌─ Web UI（单文件前端，内嵌于 binary）─────────────┐
│  模板浏览 · 参数表单 · 实时预览 · 校验面板 · 批量   │
└───────────────────────┬─────────────────────────┘
                        │ HTTP（默认绑定 127.0.0.1）
┌───────────────────────▼─────────────────────────┐
│  nctool-cli（binary 名 `nctool`）               │
│  clap 命令树 · 配置层叠加载 · 错误渲染(text/JSON)  │
│  `ui` 子命令 → 内嵌轻量 HTTP 服务器 + 静态资源     │
└───────────────────────┬─────────────────────────┘
                        │ 依赖
┌───────────────────────▼─────────────────────────┐
│  nctool-core（已有，不改架构）                    │
│  模型/校验/注册表/机床预设/生成管线                │
└───────────────────────┬─────────────────────────┘
                        │ 依赖
                nctool-tpl（已有，解析/渲染）
```

- 所有真实逻辑（校验、渲染、后处理）都在 `nctool-core`，CLI 与 UI 只是**两个输入/展示面**，不复制业务逻辑。
- 前端为**单文件** `index.html`（HTML/CSS/JS，零外部运行时依赖），经 `include_str!` 内嵌进 binary，`nctool ui` 即可离线运行。

---

## 3. 技术决策（默认推荐）

| 决策点 | 推荐方案 | 理由 |
| --- | --- | --- |
| 新增 crate | `cli/`（package `nctool-cli`，binary `nctool`） | 与库解耦，独立版本/发布/测试 |
| CLI 解析 | clap（derive） | 事实标准：自动 help / shell 补全 / 版本 |
| 参数输入 | `--param k=v`（类型自动推断）+ `--params-file x.json` | serde 已有，不额外引 YAML 依赖 |
| 配置文件 | TOML（项目 `nctool.toml` 与 `~/.config/nctool/config.toml` 层叠） | 人类可编辑，先项目后全局覆盖 |
| HTTP 服务器 | 内置轻量服务器起步（tiny_http），预留迁移 axum | 本地单用户、依赖最小化；API 扩张再升级 |
| 前端形态 | 单文件 `index.html` 内嵌 | 延续偏好：单文件、响应式、暗色模式、零 CDN |
| 输出格式 | 全局 `--format text|json` | 人读 + 机器消费（CI）两用 |
| 用户模板持久化 | 优先项目本地 `templates/`；路径可经配置扩展 | 与现有 `templates/` 一致，可进 git |

> 关键开放决策见 §11，落地前需你确认。

---

## 4. CLI 命令面

```
nctool
├── templates
│   ├── list [--category 通用|铣削|车削|钻孔|机床]
│   ├── show <name>
│   └── new <name> [--category ..] [--dir ..]      # 脚手架：生成骨架 + 参数规格注释
├── inspect <template>                              # 变量提取：必选/可选 + 行列定位
├── validate <template> [--param k=v].. [--params-file f.json] [--machine id]
├── render <template> [--param k=v].. [--params-file f.json] [--machine id]
│       [--out file] [--line-numbers] [--header] [--ascii] [--strip-blank] [--lenient]
├── generate <template> ...                         # 同 render（管线后处理全开时的规范入口）
├── part generate <part.json> [--out dir]           # 零件级批量：多工序一次生成
├── machine
│   ├── list
│   └── show <id>                                   # 支持自定义机床配置
├── config
│   ├── init                                        # 生成示例 nctool.toml
│   └── show
├── ui [--host 127.0.0.1] [--port 8787] [--open]    # 启动本地 Web UI
└── completion <bash|zsh|fish|powershell>           # shell 补全
```

全局公共选项：`--machine`、`--template-dir`、`--param`（可重复）、`--params-file`、`--format text|json`、`--out`。

**参数类型推断**：`--param x=21.0` → 数值；`--param tool=D12` → 字符串；`--param coolant=true` → 布尔。校验层（`nctool-core`）再按规格二次确认，类型不符即结构化报错。

---

## 5. Web UI 功能

| 模块 | 功能 |
| --- | --- |
| 模板浏览 | 按分类列表、关键字搜索、查看源码 + 自动提取的参数表（必选/可选/默认值/行列定位） |
| 参数表单 | 由 `extract_undeclared` + 规格**自动生成**：必选高亮、可选带默认值、按类型渲染控件（数值/字符串/布尔） |
| 实时预览 | 防抖调用 `/api/render`，G-code 等宽显示，带轻量高亮 |
| 校验面板 | 错误 / 警告 / 提示分级展示，可定位到具体参数 |
| 生成选项 | 行号、头部注释、ASCII 清洗、空行清理、严格/宽松 开关 |
| 输出 | 复制到剪贴板 / 下载 .nc 文件 |
| 机床切换 | Generic / WFL M65 / INDEX MS40 / 自定义配置 |
| 高级 | 浏览器内模板编辑并保存、参数集保存/加载（JSON 命名预设）、零件级批量生成 |
| 主题 | 亮/暗切换，响应式（移动端可用） |

---

## 6. 服务端 API（`nctool ui` 提供）

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/templates?category=` | 模板列表（名称/分类/描述） |
| GET | `/api/templates/{name}` | 模板详情：源码 + 参数规格 + 提取变量 |
| POST | `/api/inspect` | 对任意模板源码提取变量（必选/可选） |
| POST | `/api/validate` | 渲染前校验，返回结构化 `ValidationReport` |
| POST | `/api/render` | 生成 G-code：参数 + 机床 + 生成选项 → 输出 + 报告 |
| POST | `/api/part/generate` | 零件级批量生成 |
| GET | `/` | 单文件前端页面 |

返回统一 `{ "ok": bool, "data"?: ..., "error"?: { kind, message } }`；前端据此渲染校验面板。

---

## 7. 阶段计划

### 阶段 0 — 脚手架与命令框架（→ `nctool-cli v0.1.0`） ✅ 已完成（2026-09-01）

- 目标：命令树全貌可用，工程化基线建立。
- 任务：新增 `cli/` crate 并接入 workspace；clap 命令树骨架；全局选项（`--machine`/`--format`/`--param`/`--params-file`/`--template-dir`）；统一错误输出（text/JSON）；`version` 子命令；CI 扩展新 crate；参数解析单元测试。
- 产出：`nctool --help` 展示完整命令树。
- 验收：参数解析单测通过；CI 五道门对新 crate 全绿；`cargo clippy -D warnings` 零告警。
- 依赖：`nctool-core` v0.1.0（不改其代码）。
- **完成情况**：全部达成。clap 命令树含 9 个子命令；19 项参数/配置单元测试；fmt/clippy/test/doc 全绿。

### 阶段 1 — 核心 CLI 能力（→ `nctool-cli v0.2.0`） ✅ 已完成（2026-09-01）

- 目标：脚本化生成 G-code 全流程可用。
- 任务：`templates list/show/new`（new 生成带参数规格注释的骨架）；`inspect`（必选/可选 + 行列 + JSON schema）；`validate`（结构化报告）；`render/generate`（全部 `GenerationOptions` + 输出文件 + 规格默认值兜底）；`machine list/show` + 自定义机床配置加载；`config init/show` + 层叠加载；缺失必选参数时交互式补全（stdin 提示）。
- 产出：一条命令从模板到 .nc 文件；可进 CI。
- 验收：golden 测试——CLI 渲染输出与 `nctool-core` 管线结果逐字节一致；`--format json` 输出 schema 稳定；配置层叠（项目覆盖全局）生效。
- 依赖：阶段 0。
- **完成情况**：全部达成。29 项集成测试（含 golden/退出码/JSON/脚手架/配置）；`--lenient` 宽松渲染已实现。
- **待办（阶段 1 收尾，可选）**：缺失必选参数时的 **stdin 交互式补全**尚未实现（当前缺失即报错退出，适合脚本；交互补全可后续补上）。

### 阶段 2 — UI 服务与前端骨架（→ `nctool-cli v0.3.0`）

- 目标：`nctool ui` 启动本地 Web UI，模板浏览可用。
- 任务：`ui` 子命令 + 内置轻量服务器（默认 127.0.0.1:8787，`--open` 打开浏览器）；单文件前端内嵌；`templates` 列表/详情 API；前端渲染列表、详情、参数表。
- 产出：浏览器打开模板库并查看参数。
- 验收：API 集成测试通过；前端列表/详情渲染正确；确认仅监听本机回环地址。
- 依赖：阶段 1 的校验/生成能力。
- **状态：未开始**（`ui` 子命令已占位）。

### 阶段 3 — UI 完整交互（→ `nctool-cli v0.4.0`）

- 目标：表单化输入 + 实时预览的完整生成体验。
- 任务：参数表单自动生成（必选/可选/默认值）；`/api/render` 防抖实时预览；校验面板（分级 + 参数定位）；生成选项开关；复制/下载输出；机床切换；暗色模式 + 响应式。
- 产出：选模板 → 填参数 → 实时看 G-code → 校验 → 下载，全链路可视化。
- 验收：E2E 手工清单逐项通过（选模板→填参→预览→校验→下载）；移动端（≤480px）可读可点、控件不遮挡预览。
- 依赖：阶段 2。

### 阶段 4 — 高级能力与发布（→ `1.0.0` 候选）

- 目标：批量、编辑、模板库扩充与正式发布。
- 任务：零件级批量生成（`Part` → 多工序，CLI + UI 双侧）；浏览器内模板编辑 + 保存；参数集命名预设保存/加载；内置模板库扩充（铣削/车削/机床特定，如面驱动热前加工、键槽铣削场景）；安全加固（模板目录路径守卫、端口绑定、依赖审计）；打包发布（`cargo install` / release 二进制、README、CHANGELOG）。
- 产出：`cargo install --path cli` 后 `nctool` 开箱即用。
- 验收：批量生成全流程（含校验失败定位）；发布包独立运行；`cargo audit` 零告警；README/CHANGELOG 与功能一致。
- 依赖：阶段 3。

---

## 8. 里程碑版本映射

| 版本 | 里程碑 | 对应阶段 |
| --- | --- | --- |
| nctool-cli 0.1.0 | 命令树框架 + 工程基线 | 阶段 0 |
| nctool-cli 0.2.0 | CLI 全功能（脚本化生成） | 阶段 1 |
| nctool-cli 0.3.0 | UI 服务 + 模板浏览 | 阶段 2 |
| nctool-cli 0.4.0 | UI 完整交互（实时预览） | 阶段 3 |
| 1.0.0（候选） | 批量/编辑/模板库/发布 | 阶段 4 |

阶段串行推进（每阶段产出可独立使用、可回滚），阶段内任务可并行。

---

## 9. 测试与质量门

- **CLI**：`assert_cmd` 集成测试调用真实二进制；golden 测试固化渲染输出；参数解析单测。
- **服务端**：API 单元 + 集成测试（不依赖真实前端）。
- **前端**：保持零外部依赖，核心逻辑在 API 层可测；关键交互用手工 E2E 清单。
- **CI 扩展**：新 crate 纳入现有 fmt / clippy / test / doc / cargo audit 五道门。
- **安全**：模板视为可信输入（沿用 `set_path_loader` 文档约定）；UI 写模板限制在配置模板目录内；服务器仅绑定回环；不拼接 shell 命令。

---

## 10. 风险与注意

- **minijinja 锁定 `~2.24.0`**：CLI/UI 不引入其他模板引擎，升级前跑全量回归。
- **依赖膨胀**：按 §3 决策保持最小依赖；每加一个 crate 都过 `cargo audit`。
- **前端复杂度**：实时预览 + 校验面板功能密集，控制在单文件内，避免引框架（延续偏好）。
- **路径安全**：`templates new` / UI 保存模板必须校验目标路径落在配置模板目录内，防目录穿越。
- **批量生成正确性**：零件级生成复用 `nctool-core` 校验管线，任一工序校验失败即整批拒绝并定位。

---

## 11. 待确认决策

以下默认推荐，如无异议按推荐执行：

1. **UI 形态**：本地 Web UI（推荐）vs 终端 TUI vs 桌面应用。
2. **binary 名**：`nctool`（推荐）——注意与仓库名 `nctool-tpl` 区分。
3. **服务器库**：tiny_http（推荐，本地单用户）vs axum（API 规模更大时迁移）。
4. **配置格式**：TOML（推荐）vs JSON。
5. **参数文件格式**：仅 JSON（推荐，零新依赖）vs 追加 YAML 支持。
