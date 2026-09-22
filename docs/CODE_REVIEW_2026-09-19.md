# nctool 代码审查报告（第四轮 · 面向新增功能）

> 审查日期：2026-09-19
> 审查对象：`rustjinja` workspace @ `343d264`（工作区干净，批次 1–4 已全部提交）
> 代码规模：18 032 行 Rust（三 crate），其中生产口径 4 961 行
> 审查方式：四路并行通读 + 关键结论逐条**回读源码复核**，不接受子代理转述
> 历史对照：`docs/CODE_REVIEW_2026-09-18.md`（第三轮）、`CODE_REVIEW_AND_DEV_PLAN.md`（09-05）、`docs/ARCHITECTURE_REVIEW.md`（09-15）
> **本轮定位**：第三轮解决的是「静默出错」，本轮解决的是**「新增功能会不会静默失效」**。
> **实施状态**：§4 的**批次 A（打通扩展接缝）已于 2026-09-19 实施完毕**（见 `CHANGELOG.md`
> 「批次五」），批次 B/C/D 仍按原优先级开放。报告正文保留审查当时的原始结论，
> 已完成项的结论按§4 内的 ✅ 标记为准。

---

## 0. 本轮基线（实测）

| 手段 | 结果 |
| --- | --- |
| `cargo test --workspace --all-targets` | **556 项**（554 通过 + 2 ignored），全绿 |
| `cargo clippy --workspace --all-targets` | **零告警** |
| 第三轮已修项回归 | 逐条复核通过：派生链不动点（`derive.rs:144-176`）、`required_if` 优先于 `var.optional`、`check_finite` 递归列表、空前缀回退、include 闭包就近优先、孤儿清单键、稀疏覆盖可清空 —— **确认修好，本轮不再重复** |
| `diff ui/index.html cli/ui/index.html` | IDENTICAL |
| `md5sum tests/golden/*.nc` | 21 份仅 **7 份不同内容**；21 份 `.report.txt` **md5 全同**（批次 B 第 3 项已处理，见 §4） |

---

## 1. 结论摘要

| 维度 | 评分 | 一句话依据 |
| --- | --- | --- |
| 架构与分层 | **8.5** | 依赖严格单向、`core` 无 I/O、AST 匹配穷尽（编译器即安全网）；扣分在 `server.rs` 混合职责 |
| **可扩展性**（本轮重点） | **6.0 → 8.0** | 审查当时：新增一个参数类型/分类/输出格式要改 5–8 个文件，**绝大多数无编译器保护**。**批次 A 实施后**四处接缝已改为穷尽匹配 + 单一来源，仅前端 `CATS` 一处手工表待纳入对拍 |
| 可维护性 | **7.5** | 报告 DTO 两份、分类表两份、机床列表两份、UI 两份；提取器有并行两套遍历 |
| 正确性 | **8.0** | 第三轮已堵住主要静默路径；本轮新增 3 条（整数越界、宽松模式派生、清单键碰撞） |
| 安全性 | **7.5** | XSS/LFI/跨站/请求体上限均已修；扣分在无读超时（P1-19 未做）、CLI 侧读文件无上限 |
| 性能 | **7.0 → 7.5** | 注册表缓存已加；**每请求全树 `stat` 经实测仅 0.12 ms，判定不值得改**；仍扣分在每 generate 三次全量 clone 与若干 O(n²) 查找 |
| 测试有效性 | **6.5 → 7.5** | 审查当时：数量足（556），但 golden 信息量只有名义 1/3、无负向用例。**批次 B 已补**负向报告基线 + 机床维度断言；仍开放：多处弱断言（`parsing.rs:363`、`lib.rs` fuzz） |
| 文档一致性 | **6.0** | 测试数量/覆盖率/golden 组数四处漂移，且 golden 刷新命令踩了自家警告的 workspace 陷阱 |

**总体：7.3 / 10**（审查当时；批次 A 实施后主要扣分项的「可扩展性」已由 6.0 升至 8.0）。
与第三轮（7.4）持平，但**风险结构变了**：第三轮的风险是「现有功能静默出错」，本轮的风险是「**新功能加进去后某一环不生效，且没人会发现**」。

**核心判断**：这是一个**架构健康、但扩展接缝粗糙**的项目。好消息是它的编译器兜底做得比多数项目好（提取器对 minijinja AST 的匹配**零通配符**，升级 minijinja 3.0 会直接编译失败而非静默吞节点）；坏消息是同一份严谨**没有延伸到业务层**——`ParamKind`、`OutputFormat`、`TemplateCategory`、`IssueKind` 这四个最可能被扩展的枚举，全部存在「字符串表 + 通配符兜底」的组合，新增变体时编译器一声不响。

**新增功能前必须先做 §4 的批次 A 与 B**，合计约 1 人日，之后每加一个功能都会省下排查时间。

---

## 2. 架构与模块划分评估

### 2.1 做得好的（不要在这些地方「重构」出新问题）

| 项 | 结论 |
| --- | --- |
| 依赖方向 | `nctool-tpl`（引擎）← `nctool-core`（模型/校验/注册表/管线）← `nctool-cli`，严格单向；`core` 无 I/O（机床配置、模板清单都以「传入数据」形式消费） |
| 提取器 AST 匹配 | `walk_stmt`（`src/extract.rs:357-499`，20 个变体）、`walk_expr`（`:509-593`，14 个变体）、`collect_template_refs_stmt`（`:150-219`）**均无 `_ =>` 兜底**。minijinja 3.0 新增 `Expr::Tuple` 时会**编译失败**——这是新增模板语法支持时最硬的保险，务必保留 |
| 单一来源 | 参数值渲染 → `ParamValue::display`；规格 JSON → `server::spec_json`；`--param` 归一顺序 → `scripts/param_parity_cases.json` 被 Rust 测试与 `check_param_parity.mjs` **双向消费**。全仓最好的防漂移设计 |
| 安全头 | `SECURITY_HEADERS`（`cli/src/server.rs:57-72`）全局统一附加（`:749-760`），新增端点**自动生效**，无需改 CSP |
| 退出码 | `CliError::exit_code`（`cli/src/output.rs:45-57`）+ `main.rs:29-35` 单一出口，新增子命令返回 `CliError` 即可 |
| CI 门禁 | 3 job 全阻断，覆盖 fmt/clippy/test/doctest/doc/audit/覆盖率/MSRV/文档链接/Node 对拍；**CI 内所有 cargo 命令均带 `--workspace`** |
| 覆盖率口径 | 已改用 `scripts/check_coverage_caliber.py`（剔除 `#[cfg(test)]` 段）判定生产口径，第三轮的 P0-1 口径失真**已解决** |

### 2.2 结构性问题

**「枚举 + 字符串表 + 通配符」三重奏`：这是本轮唯一的架构级问题。

新增一个 `ParamKind`（比如「角度」「长度+单位」）需要同步改：

| # | 位置 | 漏改后果 | 编译器会报错吗 |
| --- | --- | --- | --- |
| 1 | `core/src/model.rs:405` `matches` 的 `_ => false` | 新类型**静默拒绝一切取值** | ❌ |
| 2 | `core/src/model.rs:408` `label` | 显示空/错 | ✅（穷尽） |
| 3 | `core/src/model.rs:172` `coerce_tagged` | 反序列化行为不一致 | 视实现 |
| 4 | `core/src/manifest.rs:804` `parse_kind_name` | 模板头部 `{# PARAMS: #}` 写新类型 → 报「既不是已知类型」 | ❌ |
| 5 | `cli/src/args.rs:113` `_ => heuristic` | `--param` 归一走启发式，可能把「角度」当字符串 | ❌ |
| 6 | `cli/src/server.rs` 前端展示 | 类型筛选失效 | ❌ |

**6 处里只有 1 处有编译保护。** 同样的问题存在于：

- **`OutputFormat`**：`core/src/pipeline.rs:336` 是 `if opts.format == OutputFormat::Text { return }` 而非 `match` —— 新增第三种格式会**静默走 G-code 后处理**（加行号、清洗 ASCII），产出错误程序且无告警。
- **`TemplateCategory`**：`manifest.rs:579 classify_by_path`（字符串表）+ `server.rs:227 parse_category` + `cli.rs:132 CategoryArg` + `ui/index.html:1678 CATS`，**四份分类表**。第三轮的 P1-8（漏 `grooving`）就是这个结构的直接产物。
- **`IssueKind`**：`#[non_exhaustive]`（`validate.rs:43`）却**没有任何穷尽 match**；级别在各构造点手选（`error_kind`/`warning_kind`/`info_kind`，`:103-130`）。更危险的是 `pipeline.rs:233` 的 `downgrade_errors_except(&[NonFinite])` 是**白名单反向**——宽松模式下任何**新增的 Error 类别默认被降级**，编译器不会提醒。

---

## 3. 缺陷清单

### 3.1 P0 —— 新增功能前必须解决

---

#### P0-1｜扩展点无编译器保护，新增类型/分类/格式/问题类别时漏改即静默失效　`指纹 EXT-SEAM-NOEXHAUSTIVE`

- 位置：`core/src/model.rs:405`、`:804`；`core/src/pipeline.rs:336`、`:233`；`core/src/manifest.rs:579`；`cli/src/args.rs:113`；`cli/src/server.rs:227`；`cli/src/cli.rs:132`；`ui/index.html:1678`
- 完整清单与漏改后果见 §2.2 表格
- 影响范围：任何「新增参数类型 / 机床无关的新输出格式 / 模板分类 / 校验问题类别」的功能
- 触发条件：新增枚举变体后只改了编译器强制的那 1–2 处
- 修复（按性价比排序，前三条是必做）：
  1. `ParamKind` 的字符串解析收敛为**单点** `impl FromStr for ParamKind`（`model.rs`），`manifest.rs:804` 与将来任何解析点都调它；`matches`（`:405`）去掉 `_ => false`，改穷尽匹配 → **此后新增类型编译器会逐处报错**。
  2. `pipeline.rs:336` 改 `match opts.format { Text => …, Gcode => … }`。
  3. `IssueKind` 增加 `fn severity(&self) -> Severity` 的**穷尽**映射；`downgrade_errors_except` 改为「按 `severity()` 保留 Error」而非白名单反向。
  4. 分类表单一来源：core 导出 `Category::ALL` + `from_path()`，CLI/HTTP/前端共同消费（`parse_category` 与 `CategoryArg` 同源于此）。
  5. 加一条守卫测试：遍历 `ParamKind::ALL`，断言 `from_str(label)` 与 `parse_kind_name` 双向一致；遍历 `Category::ALL` 断言 HTTP/CLI/前端三处表一致。

---

#### P0-2｜前后端契约无对拍：`/api/part/generate` 后端无路由，前端封装是死代码　`指纹 API-ROUTE-DRIFT`

> **2026-09-19 复核更正**：本条原记为「server 模式下批量生成静默用 JS 假数据产出程序」，
> **实测不成立** —— `runBatch` 的唯一入口 `modalBatch` 在 `ui/index.html:2571-2572` 被
> `API.mode === "server"` 守卫挡住（`toast("服务模式暂不支持批量生成")`），
> 服务模式下该功能**不可达**。真实风险降为「潜伏的契约漂移」，严重度由 P0 调整为 **P1**：
> 一旦阶段 4 实现该接口、守卫随之移除，就会踩到静默用错引擎的坑。
> 本条真正需要解决的仍是**「没有任何机制对拍前后端的接口集合」**——第三轮 P1-8
> （分类表四份导致漏 `grooving`）是同一结构的产物。

- 位置：`ui/index.html:47`（头注释）、`:1542`、`:1617`（`partGenerate` 封装）、`:2406`（`runBatch`）；`cli/src/server.rs:166-176` 路由表**无此端点**
- 证据：

```js
// ui/index.html:2406 —— 直接调 API.mock，绕过 API.request
const r = await API.mock.partGenerate(...)
```

- 触发条件：启动 `nctool ui`（server 模式）使用批量生成功能
- 影响：要么 404，要么**静默用 JS 假数据产出「看起来对」的 G-code**——对机床语境这是最坏的一类失败。且这是新增端点最容易重演的漂移模式：**路由集合没有任何对拍机制**（现有的 `check_param_parity.mjs` 只对拍参数归一）。
- 修复：
  1. 立即：补路由或删声明（二选一，别留悬空契约）。
  2. 长期：把路由集合抽成一份清单（如 `scripts/api_routes.json`），Rust 侧断言 `route()` 覆盖清单、JS 侧断言封装函数覆盖清单；再把「两份 `index.html` 字节相等」也加进 CI（当前只有 param 函数被对拍）。

---

#### P0-3｜golden 基线信息量只有名义的 1/3，新增模板/机床时防线几乎失效　`指纹 GOLDEN-LOW-ENTROPY`

- 位置：`tests/golden/`（21 `.nc` + 21 `.report.txt`）、`core/tests/integration.rs:126-132`、`:207`、`:230`
- 实测：

```
md5sum tests/golden/*.nc     → 7 个唯一值，每个出现 3 次（3 个机床预设产出逐字节相同）
md5sum tests/golden/*.report.txt → 1 个唯一值，出现 21 次（恒为「校验通过：无问题」）
```

- 影响：42 个文件只含 **7 份 G-code + 1 份报告**。机床维度完全空转（`integration.rs:126-127` 注释自认「WFL 只覆盖通用模板键」）。新增一个机床配置或改后处理逻辑时，golden 拦不住任何回归；报告维度零信息量。
- 修复：
  1. 让 3 个机床预设在**行号前缀 / 程序号前缀 / 小数位**上真正有差异（当前三个预设的差异没进入所选模板的输出），或明确缩减 golden 的机床维度并在注释里写明「不覆盖」。
  2. 补 **2 组负向 golden**（故意缺参 / 类型不符），让报告维度不再是恒等字符串——`assert_golden`（`:207`）目前只能正向冻结，无法表达失败路径。
  3. `integration.rs:132` 注释写「6 内置模板 × 3 = 18」，实际 7×3=21，顺手改。

---

### 3.2 P1 —— 重要（新功能前宜清）

#### P1-1｜`ParamKind::Integer` 缺 i64 范围检查，校验放行后可产出非法程序号　`指纹 KIND-INTEGER-NO-RANGE`

- 位置：`core/src/model.rs:394`

```rust
(ParamKind::Integer, ParamValue::Number(v)) => v.is_finite() && v.fract() == 0.0,
```

只判有限性与整值性，**不判 `i64` 范围**。而同一文件的 `coerce_tagged`（`:186`）与 `as_integer`（`:246`）都已加了 `[I64_MIN, I64_MAX)` 守卫（第三轮 P2-6 修的）——**唯独 `matches` 漏掉**。
- 触发条件：参数文件/清单写 `1e20` + `kind: integer`（`fract()==0.0`、`is_finite()` 均通过）
- 后果：校验全绿 → 模板若直接插值则**静默输出 `1e20` 级程序号**；若走 `nc_pad` 则在渲染期才报「超出整数范围」（`src/filters.rs:146`）。违反项目「渲染前可发现错误」的承诺。
- 修复：`matches` 补 `v >= i64::MIN as f64 && v < i64::MAX as f64`（注意不能复用 `2^63` 那个差一写法，见第三轮 P2-4）；补边界测试。

---

#### P1-2｜宽松模式 `Derive` 仍硬失败，与文档/报告层承诺不一致　`指纹 LENIENT-DERIVE-HARDLOGIC`

- 位置：`core/src/pipeline.rs:189`（文档）vs `:236`（实现）

```rust
// :189 文档：「唯一仍然硬失败的情形是 NaN/Inf」
let derived = crate::derive::apply(&entry.params, params).map_err(PipelineError::Derive)?;  // :236
```

而 `validate.rs` 把 `DeriveFailed` 归为 Error（`:66`），`pipeline.rs:233` 的 `downgrade_errors_except(&[NonFinite])` 把它降级为 Warning → 报告层说「可放行」，`:236` 随即返回 `Err` 且**报告被丢弃**。
- 触发条件：派生规则无 `fallback` + 源参数缺失 + 调 `generate_lenient`
- 影响：与第三轮 P0-4 同族的「报告层 vs 实际行为不一致」——用户拿到一个 Err，而文档说只有 NaN/Inf 会这样。
- 修复（二选一，建议后者）：把 `Derive` 也纳入 `downgrade_errors_except` 的白名单并让 `:236` 在宽松模式下用 `fallback` 省略该参数；或**改文档与 `IssueKind::DeriveFailed` 的级别**，明确它是宽松模式下的第二个硬失败项。无论哪种，报告层与实际行为必须一致。

---

#### P1-3｜清单键规范化碰撞被静默覆盖　`指纹 MANIFEST-KEY-COLLISION`

- 位置：`core/src/manifest.rs:452-459`（`collect()` 入 `BTreeMap`）、`:546`（`normalize_key`）
- `turning\a.j2` 与 `turning/a.j2` 被归一为同键，后者**静默覆盖**前者，无告警。对照 `variables.rs:123-134` 对重复变量是**显式报错**的——设计不对称。
- 触发条件：Windows 与 Linux 混合编辑清单，或手误写了反斜杠
- 修复：插入前查重，命中即产出 warning（复用 `ResolvedMeta::warnings` 通道，P1-12 已建好）。

---

#### P1-4｜无读超时：单连接即可挂死服务 ✅ **实测确认存在，但改动无法收敛**　`指纹 SERVER-NO-TIMEOUT`（第三轮 P1-19）

- 位置：`cli/src/server.rs` 的 `serve` 请求循环（`as_reader().take(..).read_to_end` 同步读体）
- **实测确认**（2026-09-19，原始 socket 探针）：

| 步骤 | 结果 |
| --- | --- |
| 基线 `/health` | 1.0 ms |
| 建立一条连接，只发 `Content-Length: 1048576` 头、**不发体** | — |
| 此时另一次 `/health` | **4006 ms 超时**（客户端 4s 上限） |
| 恶意连接关闭后 | 13 ms 恢复正常 |

  即：**任意本地进程一条连接就能让整个 UI 失去响应**，不是纸面推断。

- **为什么没有顺手修**：直觉的修法是「把 `Request` 移进工作线程、由该线程读体并响应」。
  `tiny_http::Request` 确实无生命周期参数（内部是 `Box<dyn Read + Send + 'static>`），
  这一半可行；但整条链路走不通：
  1. `Ctx` 含 `RefCell<…>` + `Rc<…>`，**不是 `Send`**；
  2. `Rc` 换成 `Arc` 只解决一半 —— 用编译期探针实测 `GCodeGenerator: Send + Sync`
     **不成立**，报错点是 `nctool-core` 里的 `OnceCell<Result<Renderer, TplError>>`
     与 `OnceCell<Result<Analysis, TplError>>`（`std::cell::OnceCell` 不是 `Sync`）；
  3. 因此改动必须进入 core，把 `OnceCell` 换成 `OnceLock`/`Mutex`，并进一步验证
     `Renderer`（minijinja `Environment` + `path_loader` 闭包）是否 `Sync` ——
     这是对**已发布 crate 的并发模型**的改造，不是服务层补丁。
  4. 另外 tiny_http 0.12 **完全没有读超时 API**（已 grep 整个 crate 源码确认），
     所以即使上了多线程，也只能把"主循环永久阻塞"降级为"有限个工作线程被占满"，
     做不到真正的读超时。
- 风险面：`listen_addr` 硬拒非回环，攻击者需已是本机进程；这是它能长期开放的原因。
- **结论**：独立排期，且要先在 core 上做「`OnceCell` → `OnceLock`」的评估。
  本次已把范围与阻塞点量清楚，下次不必重新调研。

---

#### P1-5｜每请求全树 `stat` ⛔ **实测后判定：当前不值得改**　`指纹 PERF-TREE-STAT`（第三轮 P1-18）

- 位置：`cli/src/context.rs::tree_stamp`；`server.rs` 每请求调用
- **实测**（200 次取样、预热后交错采样）：

| 端点 | 中位 | p95 |
| --- | ---: | ---: |
| `/health`（不建注册表） | 1.04 ms | 18.1 ms |
| `/api/templates`（建注册表，含全树 stat） | 1.16 ms | 17.0 ms |
| **差值（全树 stat 的净成本）** | **0.12 ms** | — |

  当前模板规模下，整棵树 `stat` + 缓存命中判定的净成本是 **0.12 ms**，
  相对 ~1 ms 的基线开销可以忽略。按线性外推，模板数增加两个数量级才会到 ~12 ms。
- ⚠️ **原报告建议的修法（"指纹只取根目录 + 顶层子目录 mtime"）是错的**：目录 mtime
  只在增删/改名时变化，**就地编辑文件内容不改父目录 mtime** —— 只看目录会让缓存
  返回过期模板，把一个性能问题换成正确性问题。
- **结论**：维持现状。真要做，正确方向是 TTL（限速而非省掉）或显式刷新入口，
  两者都改变"编辑后立即生效"的语义，需要产品取舍。

---

#### P1-6｜文档里的 golden 刷新命令漏 `--workspace`，刷新会静默失效　`指纹 DOC-GOLDEN-NOWORKSPACE`

- 位置：`README.md:586`、`docs/CONTRIBUTING.md:153`

```
NCTOOL_UPDATE_GOLDEN=1 cargo test    # 刷新基线
```

golden 测试在 `core/tests/integration.rs`（core 包）。裸 `cargo test` **只跑根 crate**，golden 测试根本不执行——正好踩中本项目自己在文档和长期记忆里反复警告的 workspace 陷阱，且**无任何报错**（命令成功、零文件变更）。
- 影响：新增模板后按文档刷新基线 → 以为刷新了 → CI 红。一行改动，性价比最高。
- 修复：改 `cargo test --workspace`（两处）。

---

#### P1-7｜四处重复实现，新增功能时必然复制粘贴　`指纹 DUP-FOUR-SITES`

| 重复内容 | 位置 | 建议 |
| --- | --- | --- |
| 校验报告 JSON 序列化 | `server.rs:386-409 validation_json` vs `commands/validate.rs:69-93 report_json`（**逐字段相同**） | 收敛到 core 的一个 `Serialize` DTO（`inspect` 已正确复用 `spec_json`，照它做） |
| 机床列表组装 | `server.rs:571-595` vs `commands/machine.rs:17-45` | 收敛到 `core::machine` 的一个函数 |
| 分类表 | `server.rs:227 parse_category` vs `cli.rs:132 CategoryArg` | 见 P0-1 第 4 条 |
| 模板树遍历 | `src/extract.rs:150 collect_template_refs_stmt` vs `:357 walk_stmt`（**对同一棵树的两套并行遍历**） | 合并；顺带解决 P2-9（两入口重复遍历） |

---

#### P1-8｜嵌套 include 的错误类别不细分　`指纹 ERR-NESTED-KIND-LOST`（第三轮 P2-12）

> **2026-09-19 复核后判定：不修（按设计如此）**。
>
> 原判据是「细分变体全失效，调用方无法按变体处理」。实测与代码核查后不成立：
> 1. **根因没丢**：第三轮的 ` ← ` 链修复已把内层原因（含子模板名与行号）补进消息，
>    实测 include 用例的消息里带完整根因。缺的只是**变体标签**。
> 2. **没有任何调用方需要它**：`src` 之外匹配 `TplError` 细分变体的地方只有
>    `core/src/registry.rs:1581` 的 `TemplateNotFound`，其余全部构造/透传
>    `TplError::Render`。把变体改细不会让任何调用点受益。
> 3. **改动会引入新的设计问题**：变体细化后要决定「用哪个模板名」（外层 `main.j2`
>    还是出错的内层 `sub.j2`），而 `src/lib.rs:435-470` 的测试正是把「宁缺毋错、
>    不跨模板错位恢复变量名」写成期望值的。
>
> 结论：保留现状。若将来真有调用方需要按变体分流，再连同"用哪个模板名"一起设计。

---

#### P1-9｜过滤器名被当成缺失变量名 ✅ **已修复**　`指纹 ERR-UNDEF-NAME-WRONG`（第三轮 P2-11）

> **原描述经实测不成立**：原报告称 `{{ a + missing }}` 会报首个标识符 `a`。实测该形态
> 走的是 `InvalidOperation`，归入 `TplError::Render`，**根本不提取变量名**。
> 真正可复现的是**过滤器**形态（探针逐形态验证）：

| 模板 | 错误类别 | 提取出的 variable |
| --- | --- | --- |
| `{{ missing }}` | `UndefinedVariable` | `missing` ✅ |
| **`{{ missing \| upper }}`** | `UndefinedVariable` | **`upper`** ❌ |
| **`{{ x \| f \| g }}`** | `UndefinedVariable` | **`f`** ❌ |
| **`{% if missing \| upper %}`** | `UndefinedVariable` | **`upper`** ❌ |
| `{{ missing.attr }}` / `{{ missing[0] }}` | `UndefinedVariable` | `""` ✅（宁缺毋错） |
| `{{ a + missing }}` / `{{ missing + 1 }}` | `InvalidOperation` → `Render` | —（本就不提取） |
| `{{ missing ~ "x" }}` | `UndefinedVariable` | `missing` ✅ |

- 成因：`UndefinedError` 的 span **只覆盖过滤器名**（minijinja 把错误定位到过滤器上），
  于是报「未定义变量 'upper'」——把用户指向一个过滤器名。
- 修复：span 之前（跳过空白）的最后一个字符是 `|` 时不采纳该标识符。
- ⚠️ **第三轮建议的修法（"要求标识符覆盖 trim 后整个 range"）是错的**：它挡不住
  过滤器名（span 恰好就只有 `upper` 一个标识符），却会**误伤** `{{ missing ~ "x" }}`
  （span 覆盖整条表达式，但首个标识符确实是变量）。这是本轮坚持先探针实测、
  再动手的直接价值。
- 影响：仅诊断误导，不产出错误 G-code。由 `filter_name_is_not_reported_as_undefined_variable`
  守住，并经反向验证（移除判据 → 报 `"upper"` → FAILED）。

---

#### P1-10｜CLI 侧文件读取无大小上限，与 HTTP 侧的 1 MiB 上限不对称

> ✅ **已修复（2026-09-22，批次十五）**：新增 `cli/src/limits.rs`
> （`MAX_LOCAL_TEXT_BYTES = 1 MiB` + `read_text_limited`），三处入口
> （`config.rs` / `context.rs` / `args.rs`）统一走它；先用 `metadata` 在读取前
> 拒绝超大文件，超限报 `io` 错误并带路径 / 实际大小 / 上限，**不静默截断**。
> 守卫：4 项单元测试 + 1 项 E2E（`--params-file` 超限退出码 3）。

- 位置：`cli/src/config.rs:85`、`cli/src/context.rs:199`（读全部 `.j2`）、`cli/src/args.rs:129`（`--params-file`）
- HTTP 侧已有 `MAX_BODY`（`server.rs:40`）+ 413，CLI 侧全部 `read_to_string` 无上限。本地工具定位下风险可控，但 `--template-dir` 指向网络盘/大目录时会整体读入内存。
- 修复：至少 `load_params_file` 加上限并给出明确的错误文案。

---

#### P1-11｜静默吞错与错误上下文丢失　`指纹 ERR-CONTEXT-LOST`

> ✅ **已全部收口**：
> - `registry.rs` 的 `try_iter().ok()?` **已重建为 fail-closed**（`.map_err(...)?`，
>   注释明写「一个防非法坐标的闸门不该有这种失败模式」）—— 早于本批次。
> - `RegistryError::Io` 丢路径 **已在构造处把路径并进 `io::Error` 消息**，
>   并有回归测试 `add_file_io_error_includes_the_path` —— 早于本批次。
> - `PipelineError::source()` 漏 `Derive` **已修复（2026-09-22，批次十五）**，
>   回归测试 `derive_error_is_reachable_through_source_chain`。
> - 生产 `expect` 3 处**保留**：均有不变量守护（内置模板源码应为合法、
>   `derive.is_some()` 已过滤、作用域栈由 `push_scope` 配对），改为
>   `unwrap_or` 反而会掩盖真正的编程错误；属低危，未动。

- `core/src/registry.rs:716/725` `try_iter().ok()?` —— 迭代失败时被当作「无非有限数」返回，**安全闸门静默失效**（这是防 NaN 进 G-code 的最后一道）。
- `core/src/registry.rs:336` `.map_err(RegistryError::Io)` 丢掉文件路径，`Display`（`:236`）只印 `{err}`；而 `ManifestError::Io{path,source}` 带路径 —— 两处口径不一致，用户看不到是哪个模板文件出错。
- `core/src/pipeline.rs:44-50` `PipelineError::source()` 漏了 `Derive` 变体。
- 生产 `expect`：`registry.rs:661`（`new()` 内）、`derive.rs:156`、`src/extract.rs:275`（**第三轮「unwrap/expect 仅 server.rs 3 处」的结论遗漏了这处**）。

---

### 3.3 P2 —— 改进项

**性能**
- `core/src/registry.rs:439`：`extract_params` 克隆 `entry.params` 传给 `collect_include_closure`，但返回值只取 `vars`，`specs` **用完即弃** —— 每次 `inspect` 白付一次全量 clone + O(n²) 合并。
- 每次 `generate` 三次全量 `ParameterSet` clone：`derive.rs:143`、`:146`、`pipeline.rs:171`。
- O(n²) 查找 3 处：`registry.rs:533`、`:748`、`manifest.rs:315`（参数量级下无感，列表参数变多后会显现）。

**死代码 / 冗余**
- `src/extract.rs:371` `c.declare("loop")` 永不生效（`record` 已在 `RESERVED_NAMES` 处 return）
- `src/extract.rs:250` `Collector::new(_src)` 参数未用
- `src/renderer.rs:185` `add_template_owned(name.clone(), source.clone())` 多克隆一次源码
- `core/src/derive.rs:221 derived_names`、`core/src/registry.rs:175 invalidate_analysis` **无生产调用点**；`TemplateEntry::source_text` 仍为 `pub`，「改写后须调 `invalidate_analysis`」靠调用方自觉（第三轮 P2-15 未清）

**注释与实现矛盾（后续开发危害大）**
- `core/src/pipeline.rs:293-294`：称「程序号行（`O` 开头）…已有 `N` 前缀」，未反映前缀已可配置（`program_prefix`/`line_number_prefix`）及小写 `o`/`n` 特判（`:374-377`）
- `core/src/manifest.rs:566-572`：`classify_by_path` 文档表**漏了 `grooving`**（实现 `:584` 有）—— 新增目录时照文档改会漏
- `src/filters.rs:140-145`：称 i64 上界检查防「静默输出错误程序号」，但入参先经 `f64`，`(2^53, 2^63)` 区间整数在检查前已被舍入（NC 量级不可及，属过度承诺）
- `core/tests/integration.rs:132`：写「6 内置模板」，实际 7

**文档漂移**（实测 556 项 / 89.54% 生产口径 / 93.51% 原始口径）

| 位置 | 文档写的 | 实际 |
| --- | --- | --- |
| `PROJECT_STATUS.md:23`、`CONTRIBUTING.md:84`、`CHANGELOG.md:61` | 536 项 | **556** |
| `README.md:568`、`CONTRIBUTING.md:84`、`PROJECT_STATUS.md:24` | 88.65% / 92.99% | **89.54% / 93.51%** |
| `docs/SYSTEM_DESIGN.md:43-44`、`:696` | 459 项 / 90.75% | **556 / 89.54%**（第三轮漏改这两处） |
| `docs/ARCHITECTURE_REVIEW.md:304` | 90.75% | 同上 |
| `docs/ROADMAP.md:182` | 15 组 golden | **21 组**（且有效仅 7） |

建议：文档一律不写硬数字，改指 CI job summary。

**测试质量**
- ✅ **已修（A8，2026-09-22）**：`src/lib.rs` 的 fuzz 测试不再 `let _ =`。
  `fuzz_random_templates_no_panic` / `deeply_nested_100_levels_no_stack_overflow` 改用
  共享的 `assert_extract_invariants`（名字非空/去重、行列 1 起、span 有序、未声明 ⊆ 全集）；
  `fuzz_random_render_no_panic` 改为断言返回形状（Err 必带非空消息）。
  反向验证：把 `Variable.start/end` 写反 → fuzz 立即报「span 应有序」——旧 `let _ =` 不会发现。
- ✅ **已修（批次 A）**：`tests/parsing.rs:363-366` 的 `all_math_filters_render` 已改为
  9 个过滤器逐个按数值精确断言（改后立刻抓到 `sqrt(4)` 渲染成 `"2.0"`）。
- ✅ **已修（第四轮 P2-32）**：`cli/tests/cli.rs` 的 golden 测试改读 `tests/golden/*.nc`。
- 覆盖薄弱：`ui.rs` 已 20% → **98%**；`cli.rs` 66.67%；`variables.rs` 83.70% → **97%**。

**工程化**
- ✅ **已修**：`.github/workflows/release.yml` 两处 `cargo test` 已补 `--locked` / `--all-targets`。
- `docs/DEV_PLAN_CLI_UI.md:149`：`cargo clippy -D warnings` 漏 `--workspace`（历史计划文档，建议标注为存档）
- `ui/index.html`：P2-24/25/26 仍未做 —— `--open` 先于 bind（`commands/ui.rs:16-20`）、`--port 0` 时 `browser_url` 恒显示 `:0`、Bool 参数恒提交 `false`（`:1974`，而 CLI 省略该键 → 条件必选判定可能分歧）、无 `AbortController`（后端阻塞时旧请求堆积）
- `server` 模式列表卡片只回 name/category/description（`server.rs:212-216`），缺 `builtin`/`params` → 前端恒显「示例」、必选数 0，直到 detail 拉回

---

## 3.4 审查后追加发现（推送后核对 CI 时暴露）

### P0-5｜CI 长期红着，`rust-version = "1.82"` 是对外承诺的假话 ✅ **已修复**

- 发现经过：推送后查 GitHub Actions，发现**推送前的两次提交（`343d264` / `de64e32`）CI 均为 failure**。
  逐 job 看，失败的只有 `MSRV (1.82)` 的 `Check on 1.82` 步骤，其余全绿
  （三平台质量矩阵、coverage、doc、audit 都过）。
- 本地复现（2 秒）：
  ```
  error: failed to download `clap_derive v4.6.4`
  Caused by: feature `edition2024` is required
    The package requires the Cargo feature called `edition2024`,
    but that feature is not stabilized in this version of Cargo (1.82.0)
  ```
- 根因：`clap_derive 4.6.4` 自己的清单声明 `edition = "2024"` + **`rust-version = "1.85"`**，
  是整个依赖树的最高值（次高是 `itoa` 的 1.68）。Cargo 1.82 连解析它的清单都做不到。
  实测 `cargo +1.85 check --workspace --locked` 通过 → **真实 MSRV = 1.85**。
- 为什么一直没被发现：本机门禁跑的是 Windows + stable（1.98），**MSRV 问题只在 1.85 以下暴露**；
  而第三轮加了这个 job 却没核对它的运行结果 —— job 存在 ≠ 承诺成立。
  这正是第三轮 P1-16 想解决的问题，只是当时把"加了 job"当成了"问题已解决"。
- 影响：README / CONTRIBUTING 对外承诺 1.82+，`cargo install` 到 1.82 环境的用户会直接失败；
  且 `master` 长期红着，CI 的"全绿"信号已经失效（红灯被当成常态）。
- 修复：三个 `Cargo.toml` 的 `rust-version` → `1.85`；CI job 的 toolchain / 步骤名同步；
  README / CONTRIBUTING / RELEASE / ROADMAP / TEMPLATE_INTEGRATION_PLAN 的 1.82 表述全部改掉；
  并在 CI 注释里写清"为什么是 1.85"与"抬 MSRV 前先看本 job"，避免后人以为是随手抬的。

---

### 覆盖率阈值：用 Ubuntu 实测数据结掉一个悬置的决策 ✅ **已上调 88% → 89%**

第三轮把阈值定在 88% 时留了一句话：「89.49% 是本机 Windows 实测，CI 在 Ubuntu 上跑，
余量 1.49pt 未必能跨平台兑现 —— 建议先看一次 Ubuntu 的实测数字再定」。CI 转绿后拿到了
这份数据（从 CI run 的 `rust-coverage-lcov` 产物里取 lcov，用项目自己的脚本按生产口径重算）：

| 口径 | 数值 |
| --- | ---: |
| **Ubuntu CI 生产口径**（门禁用的那个） | **4553/5040 = 90.34%** |
| Ubuntu CI 原始口径（含 `#[cfg(test)]` 段） | 9727/10343 = 94.04% |
| 本机 Windows 生产口径（同日，12:15 生成的 lcov） | 4696/5214 = 90.07% |

**跨平台差异只有 0.27pt** —— 第三轮担心的"余量未必兑现"不成立，反而更高。
于是把阈值上调到 **89%**：余量 1.34pt ≈ 68 行未覆盖生产代码，是实测跨平台差异的约 5 倍。

**刻意不设 90%**：余量只剩 0.34pt ≈ 17 行，任何一次小改动都可能误触。而"经常误报的门禁
会被当成噪音忽略"—— 本轮刚诊断的 `msrv` job 长期红着没人看，就是这个失效模式的实例。
一个总是误报的 90% 门禁，比一个可靠的 89% 门禁价值更低。

**顺带修好一个可用性缺口**：`scripts/check_coverage_caliber.py` 原先只对**相对路径**套用
`--root`，绝对路径直接使用 —— 于是 CI 产物（runner 上是
`/home/runner/work/<repo>/<repo>/cli/src/args.rs`）在本地根本无法分析（报
"源码文件不存在，无法判定生产口径"）。现新增 `--strip-prefix`，剥掉前缀即可映射到
本仓库；用法已写进 `docs/CONTRIBUTING.md`。此前要分析 CI 覆盖率只能手工改写 lcov 的
SF 路径（本次就是先这么做的），属于"有数据但拿不到"的隐性障碍。

---

## 4. 新增功能前必须优先解决的关键遗留问题

排序原则：**先解决「会让新功能静默失效」的，再解决「真实缺陷」，最后解决「整洁性」**。

### 批次 A：打通扩展接缝 ✅ **已完成（2026-09-19）**

这一批做完，此后新增参数类型/输出格式/问题类别/分类时，**编译器会逐处报错提醒你漏改**。
改动 9 个文件 +319/−66，**零运行时行为变化**（全量 557 项测试未改一行即全绿）。

| 项 | 落地方式 | 守卫 / 反向验证 |
| --- | --- | --- |
| 1. `ParamKind` 解析单点化 | `impl FromStr for ParamKind`（`aliases` 穷尽 + `label`）；`manifest.rs::parse_kind_name` 改为转发 | `param_kind_registry_is_complete` |
| 2. `matches` 去 `_` 兜底 | 改按 `self` 穷尽展开（`core/src/model.rs`） | 逐值语义与原实现一致，既有类型测试全绿 |
| 3. `OutputFormat` 后处理 | `if == Text` → 穷尽 `match`（`core/src/pipeline.rs`） | 新增格式必须显式表态 |
| 4. `IssueKind` 保留集合 | 新增穷尽的 `is_hard_fail()` + `downgrade_soft_errors()`；管线改走新路径，旧 `downgrade_errors_except` 保留但文档指向新入口 | `downgrade_soft_errors_keeps_hard_fail_only`（并断言与旧路径等价） |
| 5. 分类表收敛到 core | `TemplateCategory::{ALL, aliases, dir_names, from_dir_name, FromStr}`；HTTP `parse_category`、`classify_by_path`、`CategoryArg::from_core` 全部改为转发 | `category_arg_covers_every_core_category`；`parse_category_covers_every_core_variant` 改为遍历 `ALL` |
| 6. 顺手：golden 刷新命令 | `README.md` / `CONTRIBUTING.md` 补 `--workspace`（P1-6） | — |

> **前端 `CATS`（`ui/index.html:1678`）仍是一份手工表**：两份 HTML 的字节相等已有机制可依
> （`cli/tests/cli.rs` + `check_param_parity.mjs`），但「分类集合」尚未纳入对拍，
> 建议与批次 B 的路由集合对拍一起做。

### 批次 B：修好防线的「信息量」（约 0.5 天，必做）⚠️ **第 1、2 项已完成（2026-09-19）**

新功能加进去后，现有基线拦不住回归——先让基线有牙齿。

| 项 | 状态 | 落地方式 |
| --- | --- | --- |
| 1. 路由集合对拍 | ✅ | 新增 `scripts/api_routes.json` 作单一来源：后端 `api_routes_are_routable`（cargo test）逐条断言「不是未知接口」；前端 `scripts/check_api_parity.mjs` 断言 HTML 里每个 `/api/...` 字面量都已登记；已接入 CI（`ci.yml` 与 param 对拍并列）。`/api/part/generate` 作为**显式豁免**登记在 `frontend_only`，并写死解除条件 |
| 2. `/api/part/generate` 悬空契约 | ✅ | 复核后确认：服务模式入口被 `API.mode` 守卫挡住，**不可达**（严重度由 P0 降为 P1）。`runBatch` 由直调 `API.mock` 改为走 `API.partGenerate` 出口 —— demo 模式行为不变，服务模式拿不到结果时明确报错而非静默用另一套引擎产出 |
| 3. golden 信息量 | ✅ **已完成** | 见下方说明 |

**关于 golden 机床维度**：复核后确认「3 个预设输出逐字节相同」**不是测试写错，而是该维度在现有模板集下不含信息** —— 三个预设只在 `max_spindle_rpm` / `machine_type` / `axes` / `vendor` / `model` 上不同，而这些键**没有任何内置模板引用**；模板真正用到的键全部来自共享的 `generic_config()`。

因此没有去"制造差异"（那等于凭空发明机床编程约定），而是：
1. 把这份偶然的重复变成**受守的断言** `machine_dimension_is_currently_flat`：任一预设改了模板可见的键就红，并提示维护者"机床维度开始分化，请改成真正分维并复核 `_wfl` / `_index` 基线"；同时拦住"为消重而删掉重复基线"这种改法（删了就没人拦得住预设改动）。
2. 补 **3 组负向 golden**（`neg_missing_required` / `neg_type_mismatch` / `neg_out_of_range`），冻结失败路径的报告文本 —— 此前 21 份报告恒为「校验通过：无问题」，报告维度只有 1 份信息量。现在基线总数 45（21 正向 ×2 + 3 负向）。
3. 修正过期注释（`integration.rs` 写"6 内置模板"实为 7）并同步 README / PROJECT_STATUS / ROADMAP 的 golden 数量。

### 批次 C：三处静默正确性 ✅ **已完成（2026-09-19）**

| 项 | 落地方式 | 守卫 / 反向验证 |
| --- | --- | --- |
| 1. `Integer` 补 i64 范围检查 | `matches` 与 `as_integer` 同口径（`[I64_MIN, I64_MAX)`） | `integer_kind_rejects_out_of_i64_range`（含"下界仍放行"的防误伤断言） |
| 2. 宽松模式派生失败对齐 | `is_hard_fail()` 把 `DeriveFailed` 并入硬失败；新增 `ValidationReport::has_hard_fail()`，管线据此判定（不再写死 `NonFinite`）；文档承诺同步改为"两类" | `generate_lenient_treats_derive_failure_as_hard_fail`（断言返回 `Validation` 且**报告交给调用方**）、`downgrade_soft_errors_keeps_hard_fail_only` |
| 3. 清单键碰撞告警 | `TemplateManifest` 增 `duplicates` 字段与 `duplicate_keys()`，`from_entries` 记录同义键冲突；`cli/src/context.rs` 与孤儿键一并提示 | `normalized_key_collision_is_reported`（含"无冲突不刷噪声"） |
| 4. 有限性闸门 fail-closed | `find_non_finite` 返回 `Result<Option<String>, String>`，遍历失败不再被 `ok()?` 折叠成"没找到"，`ensure_finite_context` 按拒绝渲染处理 | 无法构造遍历失败场景，属**读码确认**（未反向验证） |

> 四处修复均经反向验证：临时还原实现后 4 项测试全部 FAILED，恢复后全绿。

### 批次 D：服务层与去重 ✅ **服务层 P1-10/P1-11 已收口（2026-09-22，批次十五）；P1-4/P1-5 已定量判定不修**

> **2026-09-22 补做**：批次的最后两个真开放项（P1-10 CLI 文件读取上限、
> P1-11 的 `PipelineError::source()` 漏 `Derive`）已随 `CHANGELOG.md`「批次十五」
> 完成；P1-11 另两项（`try_iter().ok()?`、`RegistryError::Io` 路径）经核实早于本批次
> 已修，不再开放。详见下方 §3.2 的 ✅ 标记。

**已完成**

| 项 | 落地方式 | 守卫 |
| --- | --- | --- |
| 报告 DTO 收敛（P1-7a） | 新增 `core::validate::ValidationReportJson` + `ValidationLevel::as_str()`（形状的唯一来源），CLI 侧只留 `output::report_json` 薄封装；删掉 `server.rs::validation_json` 与 `commands/validate.rs::report_json` 两份逐字段相同的实现 | `json_view_freezes_contract_fields`（core 侧冻结字段名与 level 取值） |
| 机床列表收敛（P1-7b） | 新增 `core::machine::MachineEntry` + `MachinePreset::entries()`（枚举与去重规则的唯一来源），HTTP 与 CLI 各自决定展示字段 | `entries_lists_presets_then_custom_without_duplicates` |
| 死代码（P2-10） | 删掉 `extract.rs` 的 `declare("loop")`（可证明无副作用，附说明防复发）与 `Collector::new(_src)` 的未使用参数 | 既有测试全绿 |
| 弱断言（P2-14/33） | `all_math_filters_render` 改为 9 个过滤器逐个按数值精确断言；fuzz 补提取器不变量（名字非空/去重、行列 1 起、span 有序、未声明 ⊆ 全部） | 改精确断言后立刻抓到 `sqrt(4)` 渲染成 `"2.0"` 而非 `"2"` —— 旧弱断言之所以"通过"，正因为它只看子串 |

**未做，附理由**

- **服务层读超时（P1-4）**：**2026-09-19 已实测确认缺陷存在**（一条连接即可让 `/health`
  4 秒无响应），但改动**无法收敛** —— `Ctx` 非 `Send`，且 `GCodeGenerator` 因 core 的
  `OnceCell` 缓存而**不是 `Sync`**（编译期探针确认），修它等于改造已发布 crate 的并发模型；
  tiny_http 0.12 也没有读超时 API。范围与阻塞点已量清（见 §3.2 P1-4），独立排期。
- **每请求全树 `stat`（P1-5）**：**实测净成本 0.12 ms**（`/health` 1.04 ms vs
  `/api/templates` 1.16 ms，各 200 次取样），当前规模下不值得改；原报告的"只 stat 目录"
  修法会**引入正确性问题**（就地编辑不改父目录 mtime）。判定为不修（见 §3.2 P1-5）。
- **`extract.rs` 两套并行遍历合并（P1-7d）**：**有意不做**。该文件是最安全敏感的组件
  （必选参数漏检 = 撞刀级静默错误），合并两套遍历属于"改对了没收益、改错了很难发现"的
  重构。现有分支覆盖尚可（第三轮已补齐 `collect_template_refs_stmt` 的嵌套体），
  建议保持原样，把风险留给真正需要新语法支持的时候。
- **`derived_names` / `invalidate_analysis` 可见性（P2-15）**：`nctool-core` 已发布到
  crates.io，收窄 `pub` 属破坏性变更，宜并入下一个 minor 版本一起做。

**端到端验证（真实二进制 + 真实 HTTP，2026-09-19）**

单元测试只覆盖 `route()` 与各函数，查询串里的中文百分号解码、安全头、真实进程的
退出码都不在其中。故用构建出的 `nctool.exe` 跑了一轮冒烟：

| 检查 | 结果 |
| --- | --- |
| `machine list`（文本） | `generic / wfl_m65 / index_ms40` 三行，格式与重构前一致 |
| `machine list --format json` | 字段仍为 `id/vendor/model/builtin`（有意不带 `config`） |
| `validate drill_cycle --param x=21 --format json` | 报告形状与两份旧实现逐字段一致（`level/param/message`、`template/ok/errors/warnings/issues`），退出码 1 |
| `validate` 文本 | 报告走 stdout、错误提示走 stderr |
| `templates list --category 切槽` / `grooving` / `铣削` / `milling` / `通用` | 全部 200，中英文等价 |
| `templates list --category 不存在` | clap 报 `invalid value` 并列出 6 个合法值 |
| `ui --port` + `GET /health`、`/api/machines` | 正常；机床带完整 `config` |
| `?category=切槽`（P1-8 原始 bug 场景） | **200**（修复后）；`?category=` 200；`?category=zzz` 400 |
| `POST /api/validate`、`POST /api/render` | 报告形状一致；G-code 正常产出 |
| 安全头 | CSP / `X-Content-Type-Options` / `Referrer-Policy` 均在对 `/health` 的响应上 |

**一处更正（子代理结论为误报）**

原报告 P2-10 称 `src/renderer.rs:185` 的 `add_template_owned(name.clone(), source.clone())`
"多克隆一次源码"。复核：`name` 与 `source` 在紧随其后的 `.map_err(|err| from_minijinja_error(err, &name, Some(&source)))`
里**仍被借用**，而 `add_template_owned` 按值接管两者 —— 两个 `clone()` 都是必需的。
本条不成立，**不要按原报告去"修"**。

### 批次 G：服务层两项的实测定性（2026-09-19，纯测量与文档，无代码改动）

长期挂在「第三轮 P1-18 / P1-19」名下的两项，此前一直是"读码推断"。本轮改用探针实测，
结论一项被证实、一项被证伪，两项都不需要改代码 —— 但**都需要把结论固定下来**，
否则下一次审查还会把它们当成未决项重复调研。

| 项 | 实测结论 |
| --- | --- |
| P1-4 无读超时 | ✅ **缺陷真实**：一条只发 `Content-Length` 头不发体的连接，让另一次 `/health` **4006 ms 超时**；连接一关 13 ms 恢复。改动无法收敛（`GCodeGenerator` 非 `Sync`，根因在 core 的 `OnceCell`），已把范围与阻塞点量清 |
| P1-5 每请求全树 `stat` | ⛔ **不值得改**：净成本 **0.12 ms**（1.04 → 1.16 ms，各 200 次取样）。原报告的修法还会引入正确性问题 |

### 批次 F：错误诊断准确性（2026-09-19）

| 项 | 结论 |
| --- | --- |
| P1-9 过滤器名被当成缺失变量 | ✅ **已修**（`src/error.rs`）：span 之前最后一个非空白字符是 `|` 时不采纳。先写探针跑 16 种形态、拿到实测图谱再动手 —— 第三轮建议的"标识符须覆盖整个 range"既挡不住本缺陷、又会误伤 `{{ missing ~ "x" }}` |
| P1-8 嵌套 include 错误类别不细分 | ⛔ **判定不修**：根因已由 ` ← ` 链补进消息；`src` 之外无任何调用方匹配细分变体；细化还要决定"用哪个模板名"，收益为负 |

### 批次 E：收尾小项 ⚠️ **部分完成（2026-09-19）**

| 项 | 状态 | 说明 |
| --- | --- | --- |
| P2-24 `--open` 先于 bind、`--port 0` 显示 `:0` | ✅ | `server::serve` 拆为 `bind` + `serve`：先绑定成功（失败即返回 `Err`）再开浏览器；`bind` 回读 `server_addr()` 拿实际端口。实测 `--port 0` 显示 `50199` 类真实端口、端口占用时退出码 3 且不弹浏览器 |
| P2-32 CLI golden 硬编码 | ✅ | `cli/tests/cli.rs` 的 3 个 golden 测试改为读 `tests/golden/*.nc`（新增 `read_golden`）。`program_header` 的参数对齐 fixture（补 `part_name=DEMO`），另保留一条"省略可选参数走默认值"的断言。反向验证：改基线后 **CLI 与 core 两侧都红**——此前改基线 CLI 侧照样绿 |
| P2-13 错误链截断无提示 | ✅ | 超过 `MAX_ERROR_CHAIN` 时追加 `…（错误链超过 8 层，已截断）`，避免"最后一层"被误当成根因 |
| P1-11b `RegistryError::Io` 丢路径 | ✅ | 变体形状不变（crate 已发布），在构造处把路径并进 `io::Error` 的消息；补测试断言消息含文件名 |
| 文档残留"15 组 golden" | ✅ | `PROCESS_CHECKLIST` / `ROADMAP`（3 处）改为 21 组正向；`DEV_PLAN_CLI_UI` 的 `cargo clippy` 补 `--workspace` |
| `release.yml` 缺 `--locked` | ✅ | 两处 cargo test 补 `--locked` / `--all-targets` |

**仍开放**：UI 的 P2-25（Bool 恒提交 `false`）与 P2-26（无 `AbortController`）—— 两者都需浏览器验证，
而本机不支持 agent-browser（Windows），盲改不符合本项目"改动须实测"的标准；
`src/lib.rs` 1760 行内联测试迁移（需确认是否只用公共 API，收益不明确）。

---

## 5. 已核实无问题的方面（不要在这些地方「修」出新问题）

| 项 | 结论 |
| --- | --- |
| 提取器 AST 穷尽性 | `walk_stmt`/`walk_expr`/`collect_template_refs_stmt` **零通配符**；minijinja 3.0 新增 `Expr::Tuple` 会直接编译失败。唯一 `_ => {}` 在 `declare_locals`（`extract.rs:353`），只吞 `{% set ns.x = 1 %}` 这类写目标，忽略正确 |
| 作用域模型 | for/with/set-block/macro/for-else 的帧管理与 VM 逐条核对一致；未见「必选误判为可选」（危险方向） |
| 数值过滤器边界 | `filters.rs` 全部查 `is_finite`；`nc_signed` 舍入后判零正确；`nc_pad` 的 `>=` 与宽度/小数/负数检查正确 |
| 派生链 | `derive.rs:144-176` 不动点拓扑正确，优先级「已算出的派生值 > 调用方值 > 规格默认值」，成环返回 `Circular` 且不静默取值 |
| `core` 无 I/O | 依赖单向成立，机床/清单数据都以传入形式消费 |
| CI 门禁 | 3 job 全阻断；CI 内所有 cargo 命令均带 `--workspace`；覆盖率口径已修正（89.54%） |
| 安全基线 | XSS 插值点已全覆盖（`esc` 补引号）、符号链接环已断、非回环硬拒、跨站校验 `Origin`+`Sec-Fetch-Site`、请求体 1 MiB 上限、两份 `index.html` 当前字节相同 |
| 生产 panic 面 | `src/` 0 处 `unsafe`、0 处 `panic!`；生产 `expect` 仅 4 处（见 P1-11） |

---

## 6. 与第三轮的关系

| 第三轮项 | 本轮状态 |
| --- | --- |
| P0-1 覆盖率口径 | ✅ 已解决（`check_coverage_caliber.py`），仅剩文档数字未同步 |
| P0-2 / P0-3 / P0-4 | ✅ 已修且回归通过 |
| P1-1 ~ P1-13（除 18/19） | ✅ 已修 |
| P1-18 全树 stat / P1-19 读超时 | ❌ 仍存在 → 本轮 P1-5 / P1-4 |
| P1-14 文档矛盾 | ⚠️ 部分（CI/RELEASE 已改，数字仍漂移） |
| P2-11 / P2-12 / P2-24 / P2-25 / P2-26 | 本轮 P1-9 / P1-8 / P2 逐项处理：P2-11 ✅ 已修（实际形态是过滤器名，非原描述的 `a + missing`）；P2-12 ⛔ 判定不修（根因已在消息里、无调用方需要细分变体）；P2-24 ✅ 已修；P2-25 / P2-26 ❌ 仍开放（需浏览器验证） |
| P2-14 / P2-31 / P2-32 / P2-33 | 审查当时仍存在 → 均已清：P2-31 升级为 P0-3（批次 A 修）；P2-14/P2-33 弱断言已改精确断言（批次 B）+ fuzz 不变量（A8）；P2-32 CLI golden 改读基线（批次 E） |
| P2-15 / P2-23 可见性收敛 | ❌ 仍存在 → 本轮 P2 |

**本轮新增的高价值项集中在第三轮没覆盖的维度：扩展接缝（P0-1）、前后端契约漂移（P0-2）、基线的实际信息量（P0-3）。** 这三类的共同点是——它们不会让现有功能出错，但会让**下一个功能加得心惊胆战**。
