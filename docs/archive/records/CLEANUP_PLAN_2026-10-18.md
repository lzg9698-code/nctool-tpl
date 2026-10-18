> 历史记录：原路径 `docs/CLEANUP_PLAN_2026-10-18.md`。保留当时的判定、批次与验证结果，不代表当前源码已重新验收。当前能力见[项目状态](../../PROJECT_STATUS.md)，归档导航见[历史索引](../README.md)。

# 彻底清理方案（2026-10-18）

对象：`master` @ `2729d47` 加当前未提交工作树。本页最初是待执行方案；**2026-10-18 已按 B1–B6 执行并逐批验证**，执行结果见第 12 节。本页本身仍是顶层临时文档，收尾时应归档到 `docs/archive/records/`，避免成为新的残留。

范围按用户 2026-10-18 的决定收敛：

- 根目录本地数据（`templates/`、`parameters/`、`memory/`、`presets/`、`node_modules/`、`.nctool/`）**不纳入**本轮删除，只做盘点。
- 顶层 `docs/` 的历史阶段报告**归档到 `docs/archive/`**，不直接销毁。
- 引擎公共 API `extract_member_accesses` **不动**，列为后续独立任务（见第 8 节）。

---

## 0. 结论摘要

| 类别 | 数量 | 处理 |
| --- | --- | --- |
| 无消费者的测试夹具 `tests/golden/` | 47 个文件 / 71 KB | 删除（归档包内逐字节保留） |
| 重复的生成产物 `ui/dist/` | 14 个文件 / 955 KB | 取消跟踪并忽略，由构建生成 |
| 历史阶段报告 | 13 份 | 移入 `docs/archive/records/` 并改写链接 |
| 只含“历史入口”的占位文档 | 6 份 | 删除，并入 `docs/README.md` 一段替代说明 |
| 已失效的一次性浏览器脚本 | 1 个 | 删除并清理 3 处引用 |
| 未被任何样式表导入的 CSS | 2 个文件 / 227 行 | 合并必要令牌后删除，修复编辑器样式 |
| 未定义但被使用的 CSS 变量 | 1 个（`--surface-muted`） | 补定义 |
| 工作流名称与产品版本不一致 | 2 个 | 改名 |
| `.gitignore` 缺项 | 2 条 | 补 `/presets/`、`/ui/dist/` |

清理后顶层文档由 34 份降到 15 份；受版本控制的生成资源、无消费者夹具与死脚本归零。

---

## 1. 判定标准与证据

每条结论都在当前工作树核对过，不是凭文档推断：

| 判定 | 方法 | 结果 |
| --- | --- | --- |
| `tests/golden/` 无消费者 | 在 `.rs/.py/.mjs/.cjs/.ts/.tsx/.yml` 中检索 `tests/golden` | 0 命中；最后一次改动是 2026-09-22 `65cc690`（机床配置编辑） |
| `tests/golden/` 可恢复 | 与 `docs/archive/legacy/rust-nc-removal-20261016.tar.gz` 逐成员比对 | 47/47 字节一致 |
| `ui/dist/` 是重复产物 | `diff -r ui/dist cli/ui`（排除 `assets.rs`、`source.sha256`） | 完全一致 |
| `browser_ui_refactor.cjs` 已无法运行 | 脚本头部自述“一次性、不纳入 CI”；其选择器 `切换深色主题/切换浅色主题` 在 `ui/src` 中 0 命中 | 当前 App 无主题切换入口 |
| `tokens.css` / `editor.css` 未进入构建 | `grep` 导入链；`ui/dist/assets/*.css` 中 `--editor-*`、`syntax-keyword` 均为 0 命中 | 两个文件是死文件 |
| 编辑器样式实际缺失 | `Editor.tsx` 使用 16 个 `var(--…)`，其中 11 个在已发布 CSS 中未定义 | 见第 5 节 |
| 历史文档可归档 | 13 份均被现行文档或归档索引引用，移动后逐条改写即可 | 见第 4 节映射表 |

---

## 2. 第一批：删除确定的死文件（低风险）

### 2.1 `tests/golden/`（47 文件）

- **判据**：无任何测试、脚本或工作流读取；内容全部是已移除的机床/工序 golden 输出与校验报告。
- **动作**：`git rm -r tests/golden`。
- **恢复**：`docs/archive/legacy/rust-nc-removal-20261016.tar.gz` 内 `tests/golden/**` 逐字节一致，按归档恢复流程取回即可。
- **不影响的检查**：`scripts/tests/test_rust_nc_archive.py` 只读 tar 与 manifest，不读工作树。

### 2.2 `ui/dist/`（14 文件，取消跟踪）

- **判据**：`ui/dist` 是 `node scripts/build_ui.mjs` 的中间产物，与 `cli/ui` 逐字节重复；AGENTS.md 已声明它属于生成资源、不得手改，但当前仍被 Git 跟踪，自相矛盾。
- **动作**：
  1. 在 `.gitignore` 增加 `/ui/dist/`；
  2. `git rm -r --cached ui/dist`（保留本地文件，避免打断当前环境）；
  3. 在 `docs/CONTRIBUTING.md` 的构建段补一句：新克隆需先 `node scripts/build_ui.mjs`，`--check` 才有可比对的 `ui/dist`。
- **注意**：`scripts/check_v2.py` 会扫描 `ui/dist/assets`，CI 在它之前已执行 `node scripts/build_ui.mjs`，因此不受影响；本地直接跑 `check_v2.py` 前也必须先构建（`build_ui.mjs` 已有明确报错）。

### 2.3 一次性浏览器脚本

- **文件**：`scripts/browser_ui_refactor.cjs`。
- **判据**：脚本头部自述不纳入 CI 与默认验收；`check_v2.py` 不再检查它；其依赖的主题切换按钮在当前 `App.tsx` 中不存在，无法通过。
- **动作**：删除文件，并同步移除引用：
  - `.agents/skills/nctool-ui-refactor/SKILL.md:47`
  - `docs/CODEX_DEVELOPMENT.md:84`（“必要时加 `scripts/browser_ui_refactor.cjs`”）
  - `docs/UI_REFACTOR_RESOURCES.md:137`（改为“脚本已删除，报告保留为本地历史证据”）
- **恢复**：`rust-nc-removal-20261016.tar.gz` 内保留原始字节。
- **保留**：`browser_v2.cjs`、`browser_review_fixes.cjs`、`browser_generic_parameters.cjs` 仍是 CI 三组回归，不动。

### 2.4 根目录空目录与缓存（本地，不进 Git）

| 路径 | 处理 | 说明 |
| --- | --- | --- |
| `.cargo/` | 删除 | 空目录，未被忽略也未被跟踪 |
| `presets/` | 删除并加入 `.gitignore` | 空目录；工作台运行时会在 workspace 根创建，语义同 `/templates/`、`/parameters/` |
| `node_modules/`（仓库根） | 可选删除 | 仅含 `.vitest-cache`，是误跑测试留下的缓存 |
| `.asset-group.nctool.lock` | 可选删除 | 0 字节咨询锁，`*.nctool.lock` 已忽略 |

---

## 3. 第二批：删除 6 份“历史入口”占位文档

这些文件现在只剩一段“能力已移除 + 替代入口”的说明，原始内容在归档包中；继续留在顶层会不断被误认为现行指南。

| 删除文件 | 原字节位置 | 现行替代 |
| --- | --- | --- |
| `docs/MACHINE_CONFIG_GUIDE.md` | 两份 tar 均有 | 第 3.1 节统一说明 |
| `docs/PROCESS_CHECKLIST.md` | 两份 tar 均有 | 同上 |
| `docs/SPECIALIZED_TURNING_PLUGINS.md` | 两份 tar 均有 | 同上 |
| `docs/LEGACY_WORKBENCH_MIGRATION.md` | 两份 tar 均有 | 同上 |
| `docs/PROJECT_SIMPLIFICATION_REVIEW_2026-10-13.md` | 两份 tar 均有 | 归档索引 |
| `docs/RUST_WORKBENCH_SIMPLIFICATION_PLAN.md` | 两份 tar 均有 | 归档索引 |

### 3.1 合并后的替代说明（写入 `docs/README.md`「历史资料」段）

> 已移除的内置能力（机床、工序、同步粗车、越程槽、轨迹、旧固定工作台迁移）不再设独立入口页。原始指南按原路径保存在 `docs/archive/legacy/*.tar.gz`，恢复方法见[历史归档索引](../README.md)；当前能力见[项目状态](../../PROJECT_STATUS.md)，领域能力通过[插件协议](../../PLUGIN_PROTOCOL.md)自行实现。

### 3.2 必须同步改写的引用

| 位置 | 现状 | 改为 |
| --- | --- | --- |
| `docs/README.md:17` | 链接三个占位页 | 上述替代说明 |
| `docs/WEBUI_USER_GUIDE.md:60` | 链接机床/专用插件占位页 | 链接 `PROJECT_STATUS.md` 与 `archive/README.md` |
| `docs/archive/README.md:21-24` | 「当前机床指南 / 当前工艺清单 / 当前验收清单」与 `../../examples/v2/process.json` 指向已删或不存在目标 | 机床与工艺两列改为 `legacy/` 归档文件；验收清单仍指向 `../UI_ACCEPTANCE_CHECKLIST.md`；失效示例改为“历史文件，当前仓库未保留” |
| `docs/archive/records/2026-10-14-workbench-retirement.md:11` | `../../LEGACY_WORKBENCH_MIGRATION.md` | 指向 `legacy/` 归档，并注明“历史文件，当前仓库未保留” |
| `docs/archive/records/2026-10-14-rust-template-simplification.md:7` | `../../RUST_WORKBENCH_SIMPLIFICATION_PLAN.md` | 同上 |
| `docs/archive/records/SPECIALIZED_TURNING_2026-10-08.md:3` | `../../SPECIALIZED_TURNING_PLUGINS.md` | 同上 |

---

## 4. 第三批：13 份历史阶段报告归档

### 4.1 移动映射

统一执行 `git mv docs/<name> docs/archive/records/<name>`：

| # | 文件 | 归档理由 |
| --- | --- | --- |
| 1 | `P1_FIXES_2026-10-15.md` | 阶段修复记录 |
| 2 | `P2_FIXES_2026-10-15.md` | 阶段修复记录 |
| 3 | `PROJECT_REVIEW_PLAN_2026-10-15.md` | 阶段审查计划 |
| 4 | `PROJECT_REVIEW_REPORT_2026-10-15.md` | 阶段审查报告（含已删除文件的行号引用） |
| 5 | `RESIDUAL_CLEANUP_PLAN_2026-10-14.md` | 上一轮清理记录，已被本页取代 |
| 6 | `SPECIALIZED_TURNING_PLUGINS_PLAN.md` | 2026-10-08 规划，能力已移除 |
| 7 | `COMMIT_MESSAGE_CORRECTION_PENDING.md` | 一次性事故记录，目标已失效 |
| 8 | `CODE_AND_FUNCTION_STATUS_2026-10-17.md` | 单日现状快照 |
| 9 | `OPTIMIZATION_LOG_2026-10-17.md` | 单日优化日志 |
| 10 | `P3_RELEASE_CANDIDATE_2026-10-17.md` | v2.2.0 已发布，候选身份仅具历史意义 |
| 11 | `NC_REMOVAL_IMPLEMENTATION_2026-10-16.md` | 移除实施记录 |
| 12 | `NC_REMOVAL_RECOVERY_2026-10-16.md` | 移除恢复说明 |
| 13 | `DEVELOPMENT_PLAN_2026-10-16.md` | 2026-10-16 范围方案，P0–P3 已完成 |

保留在顶层（活文档）：`README`、`ARCHITECTURE`、`SYSTEM_DESIGN`、`PLUGIN_PROTOCOL`、`TEMPLATE_WRITING_GUIDE`、`PARAMETER_MANAGEMENT`、`WEBUI_USER_GUIDE`、`UI_ACCEPTANCE_CHECKLIST`、`CONTRIBUTING`、`CODEX_DEVELOPMENT`、`RELEASE`、`PROJECT_STATUS`、`ROADMAP`、`DEVELOPMENT_BOUNDARY_2026-10-17`、`UI_REFACTOR_RESOURCES`。

> 后两份仍带日期，但分别承担“当前 API 契约”和“前端选型/兼容边界”职责，被 `docs/README.md` 与 `nctool-ui-refactor` 技能直接引用。若本轮也想归档，需同时改写技能入口；建议留到契约下一次实质变更时处理（见第 9 节）。

### 4.2 每个归档文件的头部（沿用现有 records 约定）

在文件首行前插入：

```markdown
> 历史记录：原路径 `docs/<name>`。保留当时日期、平台与结论，不代表当前源码已重新验收。当前能力见 `../../PROJECT_STATUS.md`，归档导航见 `../README.md`。
```

### 4.3 相对链接改写（逐项清单）

移动后文件位于 `docs/archive/records/`，深度为 3 层。按实际扫描结果，只有以下 6 个文件含需要改写的出站链接；其余 7 个文件的链接全部指向同组被移动文件，保持原样即可。

| 文件 | 原链接 | 改为 |
| --- | --- | --- |
| `P1_FIXES_2026-10-15.md` | `PLUGIN_PROTOCOL.md#…` | `../../PLUGIN_PROTOCOL.md#…` |
| `CODE_AND_FUNCTION_STATUS_2026-10-17.md` | `../Cargo.toml`、`../cli/…`、`../plugins/…`、`../runtime/…`、`../ui/…` | 各增加一层：`../../Cargo.toml` 等 |
| 同上 | `PROJECT_STATUS.md`、`ROADMAP.md` | `../../PROJECT_STATUS.md`、`../../ROADMAP.md` |
| `RESIDUAL_CLEANUP_PLAN_2026-10-14.md` | `CONTRIBUTING.md`、`PROJECT_STATUS.md`、`ROADMAP.md` | 各加 `../../` |
| 同上 | `archive/README.md` | `../README.md` |
| 同上 | `archive/legacy/*`、`archive/records/2026-10-14-ui-refactor.md` 等 | 分别改为 `../legacy/*`、`2026-10-14-ui-refactor.md` |
| `DEVELOPMENT_PLAN_2026-10-16.md` | `PROJECT_STATUS.md`、`ROADMAP.md` | `../../…` |
| `NC_REMOVAL_RECOVERY_2026-10-16.md` | `PROJECT_STATUS.md` | `../../PROJECT_STATUS.md` |
| 同上 | `archive/legacy/*` | `../legacy/*` |
| `SPECIALIZED_TURNING_PLUGINS_PLAN.md` | `../.agents/skills/nctool-verify/SKILL.md` | `../../../.agents/skills/…` |
| 同上 | `SPECIALIZED_TURNING_PLUGINS.md` | 该文件按第 3 节删除，改为“历史文件，当前仓库未保留” |
| `PROJECT_REVIEW_REPORT_2026-10-15.md` | `../cli/…`、`../ui/…`、`../plugins/…` | 各加一层 `../`；其中 `features/workbench/**` 已删除，按现有做法标注“历史文件，当前仓库未保留” |

锚点一律保持不变，仅调整路径深度。

### 4.4 入站引用改写

| 文件:行 | 需改链接 |
| --- | --- |
| `docs/README.md:5` | `DEVELOPMENT_PLAN_2026-10-16.md`、`P3_RELEASE_CANDIDATE_2026-10-17.md` → `archive/records/…` |
| `docs/README.md:17` | `NC_REMOVAL_RECOVERY_2026-10-16.md` → `archive/records/…` |
| `docs/README.md:35-40` | 删除 P3/NC 实施/优化记录/开发方案四行，改由归档索引统一列出 |
| `docs/PROJECT_STATUS.md:36,69,104,106,122` | 上述 13 份中出现的链接全部改到 `archive/records/…` |
| `docs/ROADMAP.md:5,21` | `CODE_AND_FUNCTION_STATUS`、`OPTIMIZATION_LOG`、`P3_RELEASE_CANDIDATE` → 归档路径 |
| `docs/CONTRIBUTING.md:39` | “当前 P0 范围、基线与验收见开发方案” → 改为指向 `ROADMAP.md` 与 `PROJECT_STATUS.md` |
| `docs/archive/records/2026-10-14-ui-refactor.md:44` 等 4 处 | 按 4.3 规则调整深度 |
| `docs/archive/README.md` | 「阶段审查与验证」表新增 13 行，列出原路径、日期、内容与当前替代入口 |

> 说明：2026-10-15 审查报告中的 `file:line` 引用在归档后仍保留原文，由头部说明其对应当时工作树。

---

## 5. 第四批：前端样式残留（含真实缺陷）

### 5.1 现状

- `ui/src/styles/tokens.css`（129 行）与 `ui/src/styles/editor.css`（98 行）**没有被任何文件导入**；`ui/src/styles.css` 只导入 `base-workbench`、`pages-surfaces`、`syntax-guide`、`workbench-overrides`。
- 构建产物 `ui/dist/assets/*.css` 中不含 `--editor-*`，也不含 `syntax-*` 类，证实两份文件未进入发行包。
- `Editor.tsx` 通过 `EditorView.theme` 使用 16 个变量，其中 11 个未定义：`--editor-background/-text/-gutter/-gutter-text/-active-line/-active-gutter/-selection/-search/-match/-error` 与 `--warning-line`。
- **附带影响**：`Editor.tsx` 的语法高亮把 CodeMirror 默认 token 映射到 `syntax-*` 语义类，而定义这些类的 `editor.css` 未导入，因此编辑器正文很可能已渲染为单色（需在浏览器目视确认，见 5.3）。这不是本轮引入的缺陷，但清理时应一并修复。
- 被导入的 `base-workbench.css:600` 使用 `--surface-muted`，但该变量只在未导入的 `tokens.css` 中定义。
- `tokens.css` 内的 `@theme inline` 是 Tailwind v4 指令，而项目没有安装 Tailwind；`data-theme="dark"` 也没有任何生产代码设置（只在测试中出现）。
- `tokens.css` 与 `base-workbench.css` 各自定义了一套 `--brand/--danger/--surface/...`，色值不同（蓝 vs 绿）。即使直接补导入也会改变现有视觉。

### 5.2 建议动作（不改变当前已验收的绿色主题）

1. **删除 `ui/src/styles/tokens.css`**：未被导入、含无依赖的 Tailwind 指令、与已发布调色板冲突、暗色主题无入口。
2. **删除 `ui/src/styles/editor.css`**：未被导入；其语法高亮规则要么并入已导入样式，要么由第 3 步直接取消语义类。
3. **修订 `ui/src/components/Editor.tsx`**：
   - 把 `--editor-*` 映射到已发布令牌：`--text`、`--surface`、`--surface-muted`、`--line`、`--brand-light`、`--warning-line`（新增）；
   - 语法高亮二选一：恢复 `defaultHighlightStyle`（删除 `syntaxClasses` 与自定义 `HighlightStyle`），或把 17 条 `syntax-*` 规则并入 `base-workbench.css`。**这条需要先做一次目视对比再定**：当前语义类没有对应样式，正文可能已退化为单色；恢复默认样式会引入 CodeMirror 自带浅色调色板，与绿色品牌不一致。建议先截图确认现状，再选择“并入规则”（保持现有配色意图）或“恢复默认”（改动最小）。
   - 删除 `Editor.tsx:19-21` 关于“主题切换不重建 EditorView”的注释（当前无主题切换）。
4. **在 `base-workbench.css` 的 `:root` 补 `--surface-muted` 与 `--warning-line`**，消除未定义变量。
5. **补 `--editor-error` 或删除其引用**（`Editor.tsx` 当前未使用 `--editor-error`，确认后仅删变量即可）。
6. 删除 `base-workbench.css` 中与 `--space` 重复或未使用的局部变量（执行时用 `postcss`/人工逐条核对，不批量删）。

### 5.3 验收

- `npm run --prefix ui lint`、`npm run --prefix ui test`
- `node scripts/build_ui.mjs` 后 `node scripts/build_ui.mjs --check`
- 隔离 home/workspace + `ui --port 0` 打开编辑器，确认：正文/行号/选区/搜索高亮可见，深色系统下不出现不可读文本；截图留证。
- 该批改动属于**视觉修正**，需在 `docs/PROJECT_STATUS.md` 记录实际验证日期与平台，不沿用历史通过结论。

---

## 6. 第五批：配置与命名一致性

| 项 | 现状 | 动作 |
| --- | --- | --- |
| `.github/workflows/ci.yml:1` | `name: NCtool 2.0 CI` | 改为 `NCtool CI` |
| `.github/workflows/release.yml:1` | `name: NCtool 2.0 release` | 改为 `NCtool release` |
| `.gitignore` | 缺 `/presets/`、`/ui/dist/` | 按第 2 节补入，并加一行注释说明来源 |
| `.gitignore:4-6` | `.workbuddy*` | 保留（可能仍被本地工具使用），仅在注释中标注“历史工具，可评估移除” |

> 工作流改名会改变 Actions 列表与徽章文字，不影响 job 语义；如需保留既有运行记录的可读性，可单独提交并在 PR 说明。

---

## 7. 执行顺序、提交粒度与验证门槛

每批单独提交（本轮不自动提交，需用户授权）：

| 批次 | 内容 | 最低验证 |
| --- | --- | --- |
| B1 | `tests/golden/`、`browser_ui_refactor.cjs`、空目录 | `cargo test --workspace --all-targets --locked`；`python -m unittest discover -s scripts/tests -p 'test_*.py'` |
| B2 | `ui/dist` 取消跟踪 + `.gitignore` | `node scripts/build_ui.mjs`；`node scripts/build_ui.mjs --check`；`python scripts/check_v2.py` |
| B3 | 6 份占位文档删除 + 入站链接 | 文档链接/锚点检查（见 7.1）；`python scripts/check_v2.py` |
| B4 | 13 份报告归档 + 链接改写 | 同上 + 人工核对归档索引表 |
| B5 | CSS/编辑器修正 | `npm run --prefix ui lint`、`npm run --prefix ui test`、`build_ui`、`--check`、`check_v2.py`、浏览器截图 |
| B6 | 工作流改名 | `actionlint`（若本地可用）或提交后由 CI 验证；不自行触发发布 |

### 7.1 文档检查脚本

建议新增 `scripts/dev/check_doc_links.py`（约 60 行，只读）：

- 扫描 `docs/**/*.md`、`README.md`、`AGENTS.md`、`.agents/**/*.md` 的相对链接；
- 校验目标存在、锚点可在目标标题中复现；
- 把“历史文件，当前仓库未保留”白名单化，避免对归档记录误报。

同时补 `scripts/tests/test_doc_links.py`，纳入 CI 的 `unittest discover`。这是本轮唯一新增的检查工具，用来防止归档后再次出现断链。

### 7.2 全量门槛（B5 之后、发布前）

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo test --workspace --all-targets --all-features --locked
cargo test --workspace --all-features --doc --locked
cargo +1.89.0 check --workspace --all-features --locked
npm run --prefix ui lint && npm run --prefix ui test
node scripts/build_ui.mjs && node scripts/build_ui.mjs --check
python scripts/check_v2.py
python -m unittest discover -s scripts/tests -p 'test_*.py'
```

---

## 8. 明确不纳入本轮

| 项 | 原因 |
| --- | --- |
| `templates/`（29 份，全部带 `metadata.nc`，28 份含 `nc.*` 执行动作） | 用户本机运行资产；应用会按退役规则拒绝，不影响构建 |
| `parameters/nc/`（13 份）、`parameters/definitions/`（128 份） | 同上 |
| `memory/`（旧会话笔记）与 `D:/ai-brain/memory/inbox/` | 按全局规则，仓库 `memory/` 只保留本地历史，不再新增 |
| `node_modules/`、`target/` | 本地依赖与构建缓存 |
| `extract_member_accesses` 及其 8 个测试、`assets/src` 与 `src/error.rs` 中的“机床”注释 | 属公共 API/文档措辞调整，需要版本与 CHANGELOG 决策；见 9.1 |
| `CHANGELOG.md` 未发布区的 59 段历史批次记录 | Keep a Changelog 允许保留；拆分需单独编辑决策；见 9.2 |
| `docs/archive/legacy/fusion/`（508 KB 补丁与指纹） | 归档原字节，按既有规则不改写 |
| `scripts/tests/test_rust_nc_archive.py` 中的 `shaft_programmer/` 守卫 | 仍是有效防回归断言 |

---

## 9. 待决策项

### 9.1 公共 API 的领域残留

`src/extract.rs:165` 的 `extract_member_accesses` 当前没有任何生产调用方，只为已移除的机床键校验服务；其注释仍写“机床完整性校验”。两个选项：

- **保留**（默认）：它是通用的“根键提取”能力，可作为插件作者工具；只需把注释改为领域中立描述，成本 5 行。
- **删除**：减少约 200 行代码与 `tests/member_accesses_adversarial.rs`，但属于 `nctool-tpl` 的破坏性变更，需在 CHANGELOG `Changed` 标注并确认无外部使用者。

建议先做“只改注释”，删除留到 2.3 或下一次 minor。

### 9.2 CHANGELOG 历史分区

`CHANGELOG.md` 共 3140 行，`[未发布]` 段包含 59 段 2026-09 起的历史批次记录（含已退役的 NC/GUI 工作）。可选方案：

- 保持现状（推荐，符合 Keep a Changelog，不丢证据）；
- 拆出 `CHANGELOG_HISTORY.md`，`CHANGELOG.md` 只保留 2.x 的版本与未发布变更；
- 在 `[未发布]` 顶部加一行“以下为历史批次记录，按时间倒序”，降低误读。

### 9.3 `DEVELOPMENT_BOUNDARY_2026-10-17.md` 与 `UI_REFACTOR_RESOURCES.md`

两份都是带日期的快照，但被当作活文档引用。若本轮也归档，需要同步修改 `docs/README.md` 与 `nctool-ui-refactor` 技能的两处链接；建议在下次组件契约或前端结构变更时一并处理。

---

## 10. 回滚

- 所有删除均为 Git 跟踪文件：`git checkout <commit> -- <path>` 即可恢复。
- 归档包内另有一份原字节：`docs/archive/legacy/rust-nc-workbench.tar.gz`、`rust-nc-removal-20261016.tar.gz`。
- `ui/dist` 取消跟踪不删除本地文件；需要恢复跟踪时 `git add -A ui/dist` 即可。
- 样式修正如需回退，按批次 revert B5 提交；不涉及数据迁移。

---

## 11. 执行前检查清单

- [ ] `git status --short` 确认用户既有改动（`.gitignore`、`AGENTS.md`、4 份 docs、`.agents/`、`.codex/`、`.mcp.json`、`CLAUDE.md`、`scripts/dev/`）未被覆盖
- [ ] 记录 `cli/ui/source.sha256` 原值
- [ ] 确认没有正在运行的本仓库 `nctool ui` 进程（删除 `ui/dist` 前）
- [ ] 大范围改动前用 `python scripts/source_baseline.py capture .nctool-backups/cleanup-20261018` 建快照
- [ ] 每批完成后立即跑该批最低验证，不把“计划执行”写成“已通过”

---

## 12. 执行结果（2026-10-18）

分支 `master`，基线 `2729d47`。执行前快照：`.nctool-backups/cleanup-20261018-b1`（338 文件）。所有改动**未提交**，工作树保留给用户审阅。

| 批次 | 实际动作 | 验证结果 |
| --- | --- | --- |
| B1 | 删除 `tests/golden/`（47 文件）、`scripts/browser_ui_refactor.cjs`；清理 3 处引用；删除空目录 `presets/`、`.cargo/` | `cargo test --workspace --all-targets --locked` 通过（24 个 test target 全 ok）；`python -m unittest discover` 40/40；`check_v2.py` PASS |
| B2 | `git rm --cached ui/dist`；`.gitignore` 增 `/ui/dist/`、`/presets/`；`CONTRIBUTING.md` 补“新克隆须先构建”说明；删除仓库根 `node_modules/`（仅含 vitest 缓存） | `build_ui.mjs` 与 `--check` 通过（14 文件），`cli/ui` 无漂移；`check_v2.py` PASS |
| B3 | 删除 6 份占位文档；改写 `docs/README.md`、`WEBUI_USER_GUIDE.md`、`archive/README.md` 与 5 份 archive 记录中的引用 | 链接检查 0 断链（除归档内历史引用）；锚点检查 0 失败；Python 40/40 |
| B4 | 13 份阶段报告 `git mv` 到 `docs/archive/records/`；自动加头部说明并改写相对链接；更新 `docs/README.md` 导航、`PROJECT_STATUS`、`ROADMAP`、`CONTRIBUTING`、`DEVELOPMENT_BOUNDARY`、`UI_REFACTOR_RESOURCES` 与归档索引 | 链接与锚点均 0 失败（剩余 24 条为归档内历史源码/`target/` 引用，已在索引注明）；Python 40/40；`check_v2.py` PASS |
| B5 | 删除死文件 `ui/src/styles/tokens.css`；`editor.css` 加入导入链；`--surface-muted`、`--warning-line`、`--editor-*`、`--syntax-*` 补入 `base-workbench.css`；`accent-color` 改用已定义令牌 | `ui lint` 通过；`ui test` 28 文件 189 项通过；构建与 `--check` 通过；隔离 home/workspace + 独立 target 二进制实测：语法高亮 4 类 token 生效、搜索面板与行号可读、`Cmd/Ctrl+F` 面板背景 `#f4f7fb`、复选框主题色 `#246e5b`；`cargo fmt`/`clippy -D warnings`/workspace 测试通过 |
| B6 | `ci.yml` → `NCtool CI`；`release.yml` → `NCtool release` | 两份 YAML 解析通过，job 结构未变 |

### 残留说明

- 顶层文档由 34 份降到 16 份（含本页）。真正意义上只剩 15 份活文档。
- `docs/archive` 内仍有 24 条指向已删除源码或本机 `target/` 的历史链接。它们属于历史记录，按既有规则不改写原文，只在 `archive/README.md` 说明“历史引用，当前仓库未保留”。
- `extract_member_accesses` 与“机床”措辞按第 9.1 节保留，未在本轮改动。
- `templates/`、`parameters/`、`memory/` 未动；本机 `templates/` 的 29 份资产仍全部带 `metadata.nc`，属用户数据。
