# 第五轮全量代码审查（2026-09-23）

> **对象**：工作区 `D:\Desktop\nctool\rustjinja`，HEAD = `417c58c`（+ 20 文件未提交改动，即
> `part generate` 与 U-12 瘦身那一批）。
> **方法**：先跑全部 10 道门禁取实测基线，再逐文件审查（优先新增/高风险面），
> 关键怀疑点**写探针实测**而不靠推理下结论。
> **与第四轮的关系**：第四轮（`CODE_REVIEW_2026-09-19.md`，3 P0 / 11 P1）**§4 已收口**，
> 本轮不重复其已修项；本轮聚焦「第四轮之后新增的功能」与「从未被审查覆盖的维度」。

---

## 0. TL;DR

| 项 | 结论 |
| --- | --- |
| 门禁 | **10 道全绿**（含最易漏的文档链接 / doc tests / cargo audit） |
| 测试 | **967 passed / 0 failed / 2 ignored** |
| 严重度 | **P0 = 0**；**P1 = 2（新增，均经探针实测）**；P2 = 5；另 11 项已核实无问题 |
| 最要紧的一条 | **JSON 极小数值静默下溢为 `0.0` 并直接写进 G-code**（P1-1）。这是本项目「静默产出错误程序」零容忍红线的**又一次复发**，且**此前各轮审查全部漏掉** |

**两条 P1 都不是新功能引入的回归，而是「一直存在、从未被任何一轮覆盖」的老洞。**
两条**均已用探针实测复现**（非推理）：P1-1 实测产出 `G0 X0.0`；
P1-2 实测非白名单输入被静默换算为 `29.61`，且白名单检查因
`check_var_values` 只遍历"被引用变量"而**根本不触发**。

**修法结论已探明**（本轮额外做了 4 轮探针，避免写出不可实施的建议）：

- P1-1 的**"在 `visit_f64` 加哨兵"这条常规思路走不通** —— `1e-400` 与 `0.0`
  解析出的 `f64` **逐位相同**，f64 通道内**不存在可用哨兵**；而 `is_subnormal`
  不能当判据（会误伤 `1e-308`…`1e-323` 这一段**合法真值**）。
  → 唯一可行落点是**解析前扫描原始 JSON 文本**，精确判据 **指数 ≤ -324**。
- P1-2 的修法很小（`compute` 里区分"未提供"与"提供了但非法"），
  **且不破坏"未选择按 DM24"的原始工艺意图**。

---

## 1. 实测基线（2026-09-23）

| 门禁 | 命令 | 结果 |
| --- | --- | --- |
| 1 UI 生成物一致 | `node scripts/build_ui.mjs --check` | ✅ md5 `125e8e16af569fb509dddfa0a20c13af`，7 片段 / 2942 行 |
| 2 参数归一 | `node scripts/check_param_parity.mjs` | ✅ 40 用例 × 2 份 UI + 后端一致 |
| 3 接口集合 | `node scripts/check_api_parity.mjs` | ✅ 前端 9 路径 / 后端 11 条 |
| 4 **文档链接锚点** | `python scripts/check_docs_links.py …` | ✅ 通过 |
| 5 格式 | `cargo fmt --all --check` | ✅ 无 diff |
| 6 Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 无代码告警（仅 Windows 增量缓存权限噪声） |
| 7 测试 | `cargo test --workspace --all-targets` | ✅ **967 passed / 0 failed / 2 ignored** |
| 8 **doc tests** | `cargo test --workspace --doc` | ✅ 1 passed / 1 ignored |
| 9 **rustdoc 零警告** | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | ✅ |
| 10 **安全审计** | `cargo audit` | ✅ 112 依赖 / 0 漏洞 |

规模：生产代码 **28394 行**（`src` 5 文件 / `core/src` 27 文件 / `cli/src` 25 文件），
集成测试 8364 行。未提交改动 20 文件 / +1484 −76。

---

## 2. P1 项（2 条，均建议尽快修）

### P1-1｜JSON 极小数值静默下溢为 `0.0`，并**直接写进 G-code** 🔴

**指纹**：`ERR-NUM-UNDERFLOW`

**位置**：`core/src/model.rs:92`（`ParamValueVisitor::visit_f64`）、`core/src/model.rs:181`（`coerce_tagged` 的 `"number"` 分支）

**性质**：**静默产出错误 G-code**，命中本项目零容忍红线。

#### 实测复现（探针，非推理）

探针 1 —— 解析层：

```
极小(紧凑)   OK  -> Some(Number(0.0))     ← {"x":1e-400}
极大        ERR -> number out of range at line 1 column 10
包裹-极小    OK  -> Some(Number(0.0))     ← {"values":{"x":{"type":"number","value":1e-400}}}
包裹-极大    ERR -> number out of range at line 1 column 45
```

探针 2 —— 端到端（该值能否被校验拦下？）：

```
渲染结果: "G0 X0.0\n"   <-- 0 直接进了 G-code
```

**没有报错，没有告警，`0.0` 就是最后的产物。**

#### 根因

`serde_json` 对数字**只在"超出 f64 上界"时报错**：

- 十进制**上溢**：`de.rs` 的 `parse_long_decimal` / `parse_any_number` 在尾数丢位且指数过高时返回
  `ErrorCode::NumberOutOfRange` → 报错。**有防护。**
- 十进制**下溢**：紧凑路径落进 `lexical::parse`，`1e-400` 得 `Ok(0.0)`；长小数路径的检查
  （见 `de.rs:750-755`）只要求"至少有一位数字"，**不检查结果是否为 0**
  → `Ok(0.0)`。**无防护。**

于是 `1e-400`（最小正规数约 `2.2e-308`，它还在其下 92 个数量级）**静默变成 `0.0`**。

而 `ParamValue` 的 `visit_f64` 是**无条件接受**：

```rust
fn visit_f64<E>(self, v: f64) -> Result<ParamValue, E> {
    Ok(ParamValue::Number(v))     // 没有 is_finite / 非零校验
}
```

`coerce_tagged` 的 `"number"` 分支同样直接 `Ok(value)`。

#### 为什么现有防线一条都没拦住

| 防线 | 为什么失效 |
| --- | --- |
| `is_finite()` 家族（`filters.rs` 全查） | `0.0` **是**有限数，放行 |
| `IssueKind::NonFinite` / `is_hard_fail` | 只拦 NaN/Inf，`0.0` 不在集合内 |
| `as_integer` / `ParamKind::Integer::matches` 的 i64 范围检查 | 只防**上溢**饱和成 `i64::MAX`，方向单一 |
| 校验层 min/max | 参数若未声明区间，不做任何检查 |

**关键观察：`0.0` 在下游是不可分辨的。** 它可能是"用户真的填了 0"，也可能是
"下溢"，**下游拿到的信息量相等**。所以必须在**解析边界**拒绝，不能指望后置检查。

#### 影响面

任何接受 JSON 的入口：`--params-file`、`POST /api/render|validate|inspect` 的 `params`、
零件定义 `part.params` / `part.ops[].params`、以及清单 `params` 里的 `default`。
用户从别的工具复制一段科学计数法（或误多打几个 0）就会命中，且**程序照常生成、照常上机床**。

> 对比：**YAML 侧（`serde_yaml 0.9.34`）没这个问题**——实测 `serde_yaml::from_str::<ParamValue>("1e-400")`
> 返回 `InvalidValue`。所以这是 **JSON 通道独有的洞**，也解释了为什么六轮审查都没撞上。

#### 建议修法（**已探明：f64 通道无法恢复字面量，故必须换判据**）

##### 探针第三轮：确认"没有哨兵可用"

我原以为可以在 `visit_f64` 里判"是否下溢"。**探针否定了这条路**：

```
1e-400    -> Number(0.0)     as_f64=0.0     is_subnormal=false  ← 与下面的真 0 逐位相同
0.0       -> Number(0.0)     as_f64=0.0     is_subnormal=false
-0.0      -> Number(-0.0)    as_f64=-0.0
1e-320    -> Number(1e-320)  as_f64=1e-320  is_subnormal=true   ← 次正规数反而"活着"
deserialize_any("1e-400") -> Ok("f64:0.0")                       ← 拿不到字面量
deserialize_str("1e-400") -> Err(invalid type: floating point `0.0`)  ← 字符串通道也拒绝
```

**三条硬事实**：

1. `1e-400` 与 `0.0` 解析出的 `f64` **逐位相同**，`is_subnormal` 都是 `false`
   → **f64 通道里不存在任何可用的哨兵**；
2. 真正"活着"的次正规数（`1e-320`）恰恰**是合法的**，不能一并拒绝
   （若用 `is_subnormal` 当判据，会**误伤**所有次正规数——那是真值，不是错误）；
3. `serde_json` 既不提供字面量，也不接受"以字符串形态读取数字"
   → **在本项目的解析入口内，无法事后区分**。

**结论：`visit_f64` 这条路走不通。** 能区分的只有**更上游**的两处。

##### 可行修法（按推荐度排序）

**方案 A（推荐）：解析前扫描原始 JSON 文本，拦"指数绝对值 ≥ 308 的字面量"。**

已在探针里验证可行：

```
{"x":1e-400}              -> ["1e-400"]     ✅ 命中
{"x":0.0}                 -> []             ✅ 不误伤
{"x":1.5e-9}              -> []             ✅ 不误伤
{"x":1e400}               -> ["1e400"]      ✅ 顺带覆盖上溢（虽已有报错）
{"x":-1e-400} / {"x":1E-400} -> 命中        ✅ 符号与大写 E 均可
```

- 判据：**用"指数 ≤ -324"作为必然归零线**（实测确定，见下）。朴素地取 `|指数| >= 308`
  **会误伤**：`1e-308` … `1e-323` 这一整段都是**合法的次正规真值**。

  实测边界（`1e{exp}`）：

  | 字面量 | 解析值 | 判定 |
  | --- | --- | --- |
  | `1e-307` | `1.0000000000000001e-307` | 正规 |
  | `1e-308` … `1e-323` | 次正规非零 | **真值，必须放过** |
  | **`1e-324`** | **`0e0`** | **⛔ 归零，静默** |
  | `1e-325` / `1e-400` | `0e0` | ⛔ 归零，静默 |

  原因：最小次正规数 = `5e-324`，故 `1e-324` 已不足以表示 → 归零。
  **精确判据 = 指数 ≤ -324**（尾数不同结论一致：`5e-309` / `9.9e-309` / `2e-308`
  实测均保持非零）。**若取 -308 会误伤 16 个数量级的合法取值。**
- 落点：`cli` 的 `--params-file` / `server` 的 `api_body` 在 `serde_json::from_*`
  **之前**加一道文本扫描；core 提供**纯函数**（core 不能有运行时 JSON 依赖，
  但"扫描字符串找数字字面量"不需要 JSON 库，可以放进 core 并被两端复用）。
- **已知缺陷**：朴素扫描会把**字符串内容**里的 `1e-400` 也算上
  （探针里 `{"note":"1e-400"}` 被误命中）。需跳过 JSON 字符串字面量
  （带 `\"` 转义的引号配对扫描）。**这是必须处理的**，否则"注释里提了一句 1e-400"
  就会导致解析失败。

**方案 B（最彻底）：换用保留字面量的解析器。** 如把 `--params-file` 的 JSON
解析换成 `serde_json::Value` + `arbitrary_precision` feature（该 feature 下
`Number` 内部是字符串，能保留原文）。**代价**：`arbitrary_precision` 是
serde_json 的 feature，且它会改变 `Number` 的行为（`as_f64` 仍可用但
`to_string` 保留原文）——**需实测确认对既有往返测试无影响**，
且 core 不能新增 crate、但 CLI/server 侧可以用。

**方案 C（最省事但最弱）：仅在文档中声明限制。** 明确写"JSON 参数不要用
指数绝对值 ≥ 308 的字面量"。**不推荐**——这正是本项目禁止的"文档化兜底"，
且用户不会读。

> ⚠️ **无论选哪条，都必须先做两件实测**：①~~确定精确的指数边界~~ **本轮已确定为 -324**；
> ②验证字符串跳过的正确性（含转义引号）。**修前必探**，这是本项目的既有约定，
> 本轮已四次因此避免走错方向（含最初"在 `visit_f64` 加哨兵"这条根本走不通的路）。

##### 附：为什么"上溢有防护"而不是"上溢也被静默接受"

同一探针确认 `1e400` 报 `number out of range`。这不是 serde_json 特意防了下溢，
而是**上溢在浮点里没有"可用结果"**（只能得 `inf`，而 JSON 无法表达 `inf`，
故必须报错）；**下溢却有"可用结果" `0.0`**，于是被静默接受。
理解这一点，才不会误以为"补一道 is_finite 就行"——`0.0` 本来就是 finite。

---

### P1-2｜`DeriveRule.fallback` 绕过源参数白名单：非白名单输入被**静默换算**成默认规格的值

**指纹**：`ERR-DERIVE-FALLBACK-UNVALIDATED`

**位置**：`core/src/derive.rs:190-204`（`compute`）、`templates/variables.yaml:143-144` 与 `:167-168`
（`tip_model.options` 与 `tip_depth.derive.fallback`）

**性质**：**静默产出错误 G-code**（按用户未要求的规格生成），命中零容忍红线。

#### 实测复现（本轮**已实测**，不再是推理）

探针 A —— 派生层（复刻 `variables.yaml` 的三项白名单 + 三项查表 + `fallback: 29.61`）：

```
tip_model=DM99_UNDEFINED   -> tip_depth=Some(Number(29.61))
tip_model=typo_B4          -> tip_depth=Some(Number(29.61))
tip_model=B4               -> tip_depth=Some(Number(8.51))
```

**任何查不到的值都被静默换成 DM24 的 29.61，不报错。**

探针 B —— 该场景能否真的走完校验、产出程序？构造模板头部同时声明
`tip_model`（白名单）与 `tip_depth`（派生），正文引用 `tip_depth`：

```
校验 is_ok = false
  [Error]   Missing 必选参数缺失（…）                      ← 报的是 tip_depth
  [Warning] Unused  参数集提供了该参数，但模板未引用        ← 报的是 tip_model
渲染被拦下（但拦的理由是 tip_depth 缺失，不是 tip_model 非法）
```

**关键发现：白名单检查根本没被触发。** `check_var_values`（`core/src/validate.rs:617`）
是 **`for var in vars`** —— 只遍历**模板正文实际引用的变量**。纯派生链上
（模板只写 `{{ tip_depth }}`）`tip_model` **不在 `vars` 里**，于是：

1. `spec.kind.matches` 不跑 → 类型/白名单/区间全都跳过；
2. `check_unused` 反向报一条 **Warning**（"参数集提供了该参数，但模板未引用
   —— 可能是模板选错或参数名拼写错误"），**这条警告恰好说中了，但只是警告**；
3. 派生层照样拿 `tip_model` 去查表 → 未命中 → **静默取 `fallback`**。

#### 修正后的可达性判定（比我初判更精确）

| 形态 | 是否可达 | 说明 |
| --- | --- | --- |
| `tip_model` 被模板**其他位置**引用（如 `{{ tip_model }}`） | **不可达** | 在 `vars` 里 → 走 `check_value_options` → `NotInOptions`（Error）拦住 |
| 纯派生链：模板只写 `{{ tip_depth }}` | **可达** | 白名单不触发，静默换算成 DM24 |

`machines/index_g420/dg_cal_ir9.j2` 属于哪一种，**需逐模板核实**（若正文有
`{{ tip_model }}` 就不可达）。**但这不能降低严重度**——可达性的成立只取决于
"模板是否顺带打印了那个变量"，**这是一件与安全性无关的偶然事实**。

#### 为什么值得单列

- 它是本项目**唯一一处"查不到就取默认"的兜底**。其余同类位置（`nc_pad` 拒绝小数、
  互斥参数禁 `| default(0)`、`DeriveFailed` 报 Error）**全部刻意选择了"报错"**。
- 记忆里的红线写着 **"不接受 `| default(0)` 兜底互斥分支参数——会静默产出 `Z0`"**。
  `fallback: 29.61` **语义上是同一类东西**：用一个看似合理的默认值掩盖"输入错了"。
- 最危险之处：**DM24 是最常用的规格**，落回它的输出"看起来完全正常"，
  用户没有任何理由怀疑。

#### 建议

1. **把"未命中"与"源值非法"分开**。`compute` 现在两种情况都走 `fallback`，
   但语义完全不同：源参数**缺失**（未提供）→ 用 fallback 是合理的业务意图；
   源参数**提供了但查不到** → 说明输入错了，必须报错。
   最小改动：在 `source.is_some()` 且未命中时**直接 `Err(DeriveError::NoMatch)`**，
   只让 `None`（未提供）走 `fallback`。这既保住了头部注释「未选择时按 DM24 处理」
   的**原始工艺意图**，又堵住了非法输入。
2. **更彻底**：给 `compute` 传入源参数的白名单，未命中时先判 `options` 再决定
   —— 但第 1 条已足够，且无需改签名。
3. **补两条测试**（并做反向验证）：
   - 源参数提供了非白名单值 → `derive::apply` **必须 Err**（当前实测返回 Ok）；
   - 源参数**未提供** → 仍应取 `fallback`（保住既有意图，防"修过头"）。
4. **顺带**：`check_unused` 那条 Warning 在纯派生链上是**假阳性**（`tip_model`
   明明被派生消费了）。它没有拦住问题，反而给出误导性的归因（"参数名拼写错误"）。
   建议把"被 `derive.from` 引用的参数"计入 `referenced`，使该警告不再误报。

---

## 3. P2 项（静态审查，5 条建议）

| # | 项 | 位置 | 说明 |
| --- | --- | --- | --- |
| P2-1 | `postprocess` 的 `checked_add` 溢出静默降级为"不编号" | `core/src/pipeline.rs:457-466` | `if let Some(next) = line_no.checked_add(step)` 在 `None` 时**什么都不做**（不编号、不报错、不警告），行号就此静默消失且**后续行号全部错位**。建议：溢出或超上限时给出**一次**显式告警（"已达行号上限 9999，后续行不再编号"），而不是无声跳过。现有注释已说明 `max_line_number` 是刻意"不编号"，但**用户无法从产物看出自己撞了上限** |
| P2-2 | 两个 crate 的发布包内测试必然失败 | `cli/Cargo.toml` / `core/Cargo.toml` | 第四轮已实测（cli 的 `include_str!("../../scripts/…")` 编译期断、core 的 golden 运行时断）。**均 `#[cfg(test)]` 内，不影响 `cargo build`/`install`**。三种修法已列在记忆里，**属设计取舍，本轮仍建议保持待决**，但值得在 `README` 或 `Cargo.toml` 注释里**显式记一句**"发布包不含可运行的测试资源"，免得下一个人重新排查 |
| P2-3 | `PartSpec` 缺"未知字段"拒绝 | `core/src/part.rs:44-64` | `PartSpec` / `PartOp` 都**没有 `deny_unknown_fields`**，而 `ParamOverride` / `ParamSpec` 有。字段名写错（如把 `params` 写成 `parameters`、`default_machine` 写成 `defaultMachine`）**静默忽略**。对 `ops` 这类数组字段，写错会让零件"少一道工序"却照常成功——**又一次同类静默失败**。建议加 `#[serde(deny_unknown_fields)]` |
| P2-4 | 前端对 `skipped` 工序的呈现是空面板 | `ui/src/32_script_ui.part.html:886-887` | HTTP 分支把"整体未交付但该工序本身没失败"的工序标为 `{skipped:true}` 且**不带 `output`**（设计正确）。但前端 `showBatch` 只判 `r.error`：无 `error`、无 `output` → `highlightGcode(undefined)`，用户看到**一个空白面板**，误以为"这道工序成功了但没内容"。建议显式渲染"⚠ 该工序未失败，但因其他工序失败而整体未交付" |
| P2-5 | 前端 mock 与后端的失败语义仍不完全对齐 | `ui/src/31_script_api.part.html:152-175` | mock 的 `doPart` 是**逐工序尽力而为**：坏工序进 `error`，好工序**照常给 `output`**（不回 `skipped`）。后端则整体不交付。demo 模式（`file://`）下用户会看到"部分工序有产出"的界面，切到 server 模式后变成"全空白"——**同一份零件 JSON 两种观感**。属前端 demo 的既有简化，但与本轮刚统一的语义有落差，建议在 mock 上补 `skipped` 以保持一致 |

---

## 4. 已核实无问题的方面（**不要在这些地方"修"出新问题**）

本轮逐项复查，结论如下（第四轮 §5 的结论仍然成立，此处只列本轮**新覆盖**的部分）：

| 项 | 结论 |
| --- | --- |
| 新增 `core/src/part.rs`（1163 行） | 编排逻辑正确：参数继承（`merge_params` 按名整体替换）、机床解析（三级优先 + 未知标识报错）、行号续编（**用实际末行号而非行数推算**）、事务语义（失败即整体不交付，类型上不返回 `outcome`）。**37 项测试**，边界用例（`O1001` 不编号、全段无编号行游标不动）齐全 |
| `ParameterSet` 自定义 `Deserialize` | 扁平/包裹两形态均正确，**序列化仍为包裹形式**（不破坏既有消费方），同名冲突扁平优先。9 项测试覆盖往返与互斥语义。`null` 值给出可操作提示（`core/src/model.rs:109`）。**这是本轮改得最好的地方** |
| `line_number_start` 透传 | `generate` 丢弃游标 / `generate_with_cursor` 返回游标，两条路径共用 `generate_impl`；`OutputFormat::Text` 分支**不推进游标**（`pipeline.rs:410-413`）正确；`max_line_number` 与起始值的关系已在文档里写明"起始值本身不参与上限判断" |
| CLI `part generate` 事务性 | E2E 实测：坏工序 → `exit 1` 且 `!out.exists()`（无半成品）；错误消息含全部失败工序且缩进分层。`write_out_file` 的父目录创建与 `render` 策略一致 |
| HTTP `part_generate` 逐工序结果 | 结果**按 index 对齐填回**（用 `BTreeMap` 而非 `Vec` 追加），成功工序带 `skipped:true` 且不带 `output` —— 不吐半成品。`InvalidSpec` 回 400 无 `data`，`OperationsFailed` 回 200 有 `data`，与文档/契约一致 |
| 枚举判定函数无通配兜底 | `ParamKind::matches` / `label` / `aliases` 均**穷尽匹配**（`model.rs:391-457`），新增变体会编译失败而非静默拒绝；`ALL` 由 `param_kind_registry_is_complete` 对拍 |
| `derive` 不动点与成环 | `derive.rs:150-176` 拓扑正确，成环返回 `Circular` 且**不静默取值**；`compute` 对 `List` 源值直接 `UnusableSource` 报错（不落 fallback） |
| 宽松模式的硬失败集合 | 由 `IssueKind::is_hard_fail`（`validate.rs:111-130`，穷尽匹配）决定，**非白名单**；实测 NaN 对**每一道**工序都硬失败（证明参数继承生效） |
| 生产 `panic` 面 | `src/` 0 处 `unsafe`；生产 `expect` 仅 7 处，**逐条核查均"不可能失败"**：作用域栈非空（`push/pop` 严格配对）、`derive` 已按 `is_some()` 过滤、内置模板注册、静态 `Content-Type` 头、`serde_json` 序列化固定结构。`unreachable!()` 在 `commands/mod.rs:45` 已由上层分支穷尽保证 |
| `cargo audit` | 112 依赖 0 漏洞 |

---

## 5. 未提交改动的审查意见

当前 20 文件 / +1484 −76 **可以提交**。三道脚本门禁、fmt、clippy、967 项测试、
rustdoc、audit 全绿。两点提醒：

1. `scripts/api_routes.json` **文件末尾无换行**（`\ No newline at end of file`）。
   不违规、门禁也过，但会造成后续每次编辑都产生一行无意义 diff，建议补上。
2. 提交前确认 `core/src/part.rs`（新文件）与 `ui/index.html`（构建产物）**一起入库**，
   否则门禁 1 会红。当前工作区里两者都在 `??`/`M` 列表中，符合预期。

---

## 6. 与历轮审查的关系

| 轮次 | 日期 | 结论 | 本轮状态 |
| --- | --- | --- | --- |
| 第三轮 | 09-18 | 4 P0 / 20 P1 / 37 P2 | 批次三/四已完成 |
| 第四轮 | 09-19 | 3 P0 / 11 P1 | **§4 已收口**（`P1-1 ~ P1-13` 除 18/19 全修）；仍开放 P2-25/26（前端，需浏览器） |
| **第五轮（本轮）** | **09-23** | **0 P0 / 2 P1 / 5 P2** | 新增 2 条 P1，**均为历史遗留而非新功能回归** |

**本轮的判断**：新增的 `part generate` 全链路（core/CLI/HTTP/前端/契约/测试）
质量**高于项目平均水平**——边界用例到位（`O1001` 不编号、全段无编号行、
NaN 每工序硬失败）、失败语义有文档有测试、且刻意避开了"用文本前缀做假断言"。
**两条 P1 都不在新增代码里。**

**但由此也暴露一个系统性问题**：`1e-400` 这类输入**历轮审查都没覆盖**，
因为既往各轮都聚焦"模板语义 / 参数模型 / 前后端契约 / 发布流程"，
**没有人系统地测过"解析边界的极端数值"**。建议把「数值边界对抗性测试」
（上溢 / 下溢 / 次正规数 / 超大位宽整数 / `-0.0`）补成一组**常驻测试**，
而不是靠下一次审查偶然撞见。

**另一条更值得记的方法论**：P1-2 的根因不在 `derive.rs`，而在
**`check_var_values` 的遍历口径**（只查"被引用的变量"）。
这意味着**任何"参数只参与派生、不出现在正文"的模板，其参数约束全部失效**
——白名单、类型、区间一个都不跑。这不是 `tip_depth` 一个人的问题，
而是**参数校验覆盖面的一处结构性缺口**。
建议在修 P1-2 的同时评估：**是否应对 `specs` 中所有"用户可提供"的参数做值级检查
（而非只查被引用的）**。当前口径的初衷是避免 `SpecUnused` 噪声，
但代价是**约束静默失效**——与本项目的红线相悖。

---

## 7. 建议的推进顺序

1. **P1-1 下溢静默归零**（唯一一条"输入合法但产物错误"的洞）——先探针确定修法，再动手，配反向验证。
2. **P1-2 `derive` fallback + 校验覆盖面缺口**——改动很小（`source.is_some()` 时未命中即报错），
   **且不破坏"未选择按 DM24"的原始意图**（未提供走 fallback，提供但非法则报错）。
   顺带评估 `check_var_values` 的遍历口径。
3. **P2-3 `deny_unknown_fields`**（一行改动，堵住一整类拼写静默失败）。
4. P2-1 行号上限告警、P2-4 前端 `skipped` 呈现、P2-5 mock 语义对齐。
5. 建立数值边界常驻测试组（防回归，也防止"下一次仍然漏"）。
