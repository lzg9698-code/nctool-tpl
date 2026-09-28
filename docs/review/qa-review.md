# nctool 深度代码审查报告（QA 视角）

> 审查维度：**数值正确性 / 边界情况 / 校验逻辑 / 测试覆盖**
> 审查人：严过关（QA Engineer）
> 审查日期：2026-09-26
> 工程：`D:\Desktop\nctool\rustjinja`（Rust workspace，`core` + `cli` + 根 crate `nctool-tpl`）
> 约束声明：本次为**纯只读审查**，未修改任何源码/测试文件；所有行号均经 `Read` 逐行核对。

---

## 0. 审查元信息与方法

### 0.1 审查范围

| # | 文件 | 行数 | 说明 |
|---|---|---|---|
| 1 | `core/src/json_num.rs` | 1147 | 数值保真 / f64 下溢候选提取（最高优先级） |
| 2 | `core/src/validate.rs` | 2484 | 参数校验引擎 |
| 3 | `core/src/part.rs` | 1335 | 零件级多工序编排 |
| 4 | `core/src/machine.rs` | 580 | 机床预设与配置键 schema |
| 5 | `core/src/variables.rs` | 482 | 变量库 |
| 6 | `core/src/derive.rs` | 533 | 派生参数 |
| 7 | `core/tests/*.rs`（7 个，2547 行）+ `cli/tests/*.rs`（7 个，5737 行） | — | 覆盖缺口评估 |
| 8 | 交叉引用（定位问题所必需，非独立审查对象） | — | `core/src/registry.rs`、`core/src/model.rs`、`core/src/asset/path.rs`、`src/filters.rs`、`cli/src/output.rs`、`cli/src/server.rs`、`templates/variables.yaml`、`templates/templates.yaml`、`docs/PROCESS_CHECKLIST.md` |

### 0.2 方法

1. 逐文件精读（数值路径全量、`f64` 比较/转换点全量）
2. 关键判据的**边界反演**：对每一条「拦截」逻辑，手工构造「反向绕过」输入并沿代码路径推演
3. `grep` 交叉核实（硬编码常量、调用方覆盖度、`#[ignore]`、覆盖率数据）
4. 覆盖率基线取自 `cov.log` 与 `lcov.info`（2026-09-26 生成）

### 0.3 结论摘要

- **问题总数 15 条**：P0 致命 1 条、P1 严重 6 条、P2 一般 8 条
- **头号风险（P0，Q-01）已确认成立**：主轴转速上界 6000 硬编码于 `core/src/registry.rs:951`，机床预设 `max_spindle_rpm`（WFL=3500 / INDEX=5000）**全程不参与校验**，且该键在 `core/src/registry.rs` 与 `templates/**` 中引用次数为 **0**（纯文档键）。
- **测试覆盖整体健康**：生产行覆盖 92.55%（8198/8858，见 `cov.log`），全仓仅 2 处 `#[ignore]` 且均为性能/探针类并有明确跑法说明——**不存在被静默跳过的正确性测试**。缺口集中在「跨参数/跨机床联动」「YAML 词法边界」「JSON 侧下溢确认」三个面。
- **无 `insta` 等快照框架**（`Cargo.toml` / `core/Cargo.toml` / `cli/Cargo.toml` 均无快照依赖，无 `.snap` 文件）。所有期望值是内联字面量 —— 这**规避了「基线自动接受（auto-accept）导致漂移被吞」**的风险（正面结论），代价是大规模文案改动需人工批量维护。

---

## 1. 已知风险核实：主轴转速上界 6000 硬编码（F1）

> 结论：**风险成立，且比清单记录更宽。** 清单 `docs/PROCESS_CHECKLIST.md:96 / :207` 记为「不阻断、用户自检」的 Backlog 项；实际核实发现除内置 `tool_change` 模板的静态上界外，**迁移模板里全部 RPM 类参数连静态上界都没有**（见 Q-02），因此风险面大于文档描述。

---

## 2. 问题清单

### 2.1 P0-致命

#### Q-01　主轴转速上界 6000 硬编码，与机床 `max_spindle_rpm` 完全不联动

- **文件:行号**
  - `core/src/registry.rs:951`（硬编码点）
  - `core/src/registry.rs:931`（把该值写进 G-code 的模板行）
  - `core/src/machine.rs:76`（WFL `max_spindle_rpm = "3500"`）
  - `core/src/machine.rs:96`（INDEX `max_spindle_rpm = "5000"`）
  - `core/src/machine.rs:310-313`（schema 默认 `6000`）
  - `core/src/machine.rs:405`（generic 预设 `6000`）

- **问题简述**：内置模板 `tool_change` 的 `spindle_speed` 上界写成常量 `6000.0`，校验时**从不读取**当前机床的 `max_spindle_rpm`；WFL/INDEX 预设声明的 3500 / 5000 只是一段无人消费的文档。

- **证据**

```rust
// core/src/registry.rs:944-952
crate::validate::spec(
    "spindle_speed",
    ParamKind::Integer,
    true,
    None,
    "主轴转速（S 值，正整数）",
)
.with_range(1.0, 6000.0)      // ← 静态常量，与 machine 无关
.with_unit("r/min"),
```

```rust
// core/src/registry.rs:931  （同一模板的正文，直接发射 S 值）
"{{ machine.spindle_on }} S{{ spindle_speed | nc_strip }} (主轴正转)\n",
```

```rust
// core/src/machine.rs:75-76
c.extend([
    ("max_spindle_rpm".to_string(), "3500".to_string()),   // WFL M65
```

**交叉核实（grep 结果）**：`max_spindle_rpm` 在 `core/src/registry.rs` 出现 **0 次**，在 `templates/**` 出现 **0 次**；仅在 `core/src/machine.rs`、`core/src/model.rs`、`core/src/pipeline.rs`（测试）、`core/src/asset/machine.rs`、`cli/**`、文档中出现。即：**机床侧声明了上限，模板与校验两侧都没有引用它。**

- **影响**
  - 在 `wfl_m65`（主轴上限 3500 r/min）下执行 `--param spindle_speed=5000`，校验**全绿**，输出 `M3 S5000` —— 超速 **43%**。
  - 在 `index_ms40`（5000）下 `spindle_speed=6000` 亦全绿 —— 超速 20%。
  - **CNC 物理后果**：主轴/刀柄超速 → 刀具断裂、工件飞出、主轴轴承失效，属可致人伤的机床事故。这类错误**没有任何运行时提示**（文档明确写着「本项目不阻断（用户自检）」，`docs/PROCESS_CHECKLIST.md:207`）。
  - 该缺陷同时违背了本项目自己写在 `core/src/validate.rs:905-907` 的承诺：「这些约束是 CNC 工艺安全的主要承载处：……**主轴转速上界**……全部由 `ParamSpec` 的 `min` / `max` 字段表达」——但 `max` 是一个与机床无关的常量，等于没表达。

- **改进建议（代码级）**

  1. `core/src/model.rs` 的 `ParamSpec` 增加**动态上界来源**字段：

```rust
/// 动态上界：从机床配置的某个键取值（如 `machine.max_spindle_rpm`）。
/// 与 `max` 并存：`max` 是「无机床信息时的静态兜底」，动态值优先。
pub max_from: Option<String>,
pub min_from: Option<String>,
```

  2. `core/src/validate.rs` 增加解析函数，并让 `check_value_constraints`（`core/src/validate.rs:908`）接收 `machine: Option<&MachineConfig>`：

```rust
fn resolve_bound(spec: &ParamSpec, machine: Option<&MachineConfig>, is_max: bool) -> Option<f64> {
    let (dynamic_key, static_v) = if is_max { (&spec.max_from, spec.max) } else { (&spec.min_from, spec.min) };
    let key = dynamic_key.as_deref()?;
    let raw = machine.and_then(|m| m.get(key))?;          // MachineConfig::get
    match raw.trim().parse::<f64>() {
        Ok(v) if v.is_finite() && v > 0.0 => Some(v),
        // 机床侧值非法（空/0/负数/非数字/带 toml decor 空白）：回退静态值并**报警告**，
        // 绝不静默用 0 当上界（那会把所有转速判为越界，见 Q-12）
        _ => static_v,
    }
}
```

  3. 把 `machine` 沿 `check_vars`（`core/src/validate.rs:385`）→ `check_var_values`（:640）→ `check_value_constraints`（:908）与 `check_param_values`（:701）透传；`validate_template` / `validate_with_vars` 增加 `machine` 参数（或加 `*_with_machine` 变体保持向后兼容）。

  4. `core/src/registry.rs:951` 改为：

```rust
.with_range(1.0, 6000.0)
.with_max_from("machine.max_spindle_rpm")   // 有机床时以机床值为准
```

  5. 错误消息必须点名机床：`参数值 5000 r/min 超出上界 3500 r/min（来自机床 wfl_m65 的 max_spindle_rpm）`。

  6. 同步更新 `docs/PROCESS_CHECKLIST.md:96`、`docs/PROCESS_CHECKLIST.md:207` 中 F1 的状态（由「不阻断 / Backlog」改为「已按机床校验」）。

- **应补充的测试用例**

```rust
// core/src/validate.rs tests
#[test]
fn spindle_speed_bound_follows_machine_max_spindle_rpm() {
    // (机床, S 值, 期望是否通过)
    for (mid, s, ok) in [
        ("wfl_m65",    3499.0, true),
        ("wfl_m65",    3500.0, true),   // 闭区间边界
        ("wfl_m65",    3501.0, false),  // ★ 当前实现会通过 —— 本用例必须红
        ("wfl_m65",    5000.0, false),
        ("index_ms40", 5000.0, true),
        ("index_ms40", 5001.0, false),
        ("generic",    6000.0, true),
        ("generic",    6001.0, false),
    ] { /* validate 并断言 has_kind(OutOfRange) == !ok */ }
}

#[test]
fn invalid_machine_max_spindle_rpm_falls_back_to_static_max_with_warning() {
    // max_spindle_rpm = "" / "0" / "-100" / "abc" → 不得把上界变成 0（那会全量误杀），
    // 应回退 6000 并产出一条 Warning（见 Q-12）
}
```

---

### 2.2 P1-严重

#### Q-02　全部 RPM 类参数**完全没有上界**（比 Q-01 更宽的口子）

- **文件:行号**
  - `templates/variables.yaml:100`（`U_RTRPM # 粗铣刀转速 (RPM)`，仅 `kind: integer`）
  - `templates/variables.yaml:106`（`U_FTRPM # 精铣刀转速 (RPM)`）
  - `templates/variables.yaml:127`（`R1 # 倒角刀转速 (rpm)`）
  - `templates/variables.yaml:177`（`speed_limit # 转速上限 (r/min)`）
  - `templates/templates.yaml`：全文 grep `RTRPM|FTRPM|speed|转速` → **0 命中**（清单层无任何覆盖）

- **问题简述**：12 个迁移模板引用 RPM 参数（`grep -rln "U_RTRPM\|U_FTRPM" templates/ | wc -l` = 12），但变量库只声明了类型 `integer`，未声明 `min` / `max`；清单层也没有补。

- **证据**

```yaml
# templates/variables.yaml:100-101
  - name: U_RTRPM # 粗铣刀转速 (RPM)
    kind: integer        # ← 无 min / max
```

```jinja
{# templates/machines/index_g420/uz_fkm_temp1.j2:42 #}
G97 S1={{ U_FTRPM | nc_fixed(0) }} M1=3
```

- **影响**：`--param U_RTRPM=999999` 校验全绿并输出 `G97 S1=999999`。与 Q-01 同源，但**连 6000 这道静态闸门都没有**。同理 `U_RTRF` / `U_FTF` / `R2`（进给，行 102 / 108 / 128）也没有上界，`F999999` 会被接受。

- **改进建议**
  1. 在 `templates/variables.yaml` 为 4 个 RPM 键与 3 个进给键补 `min: 1` 与 `max`（先用保守静态值，如 `max: 6000`）。
  2. 长期：改用 Q-01 的 `max_from: machine.max_spindle_rpm` 动态上界。
  3. 给 `templates/templates.yaml` 增加一条**守卫测试**：扫描变量库，断言所有名字含 `RPM|rpm|转速` 的键必须声明 `max`。

- **应补充的测试用例**：`variable_library_declares_upper_bound_for_all_rpm_params`（读真实 `templates/variables.yaml` 断言，防回归）。

---

#### Q-03　YAML 块标量只跳过**第一行**内容，第二行起被当普通标量扫描（假阳性）

- **文件:行号**：`core/src/json_num.rs:490`（判定）、`core/src/json_num.rs:492-495`（跳过 + `continue`）、`core/src/json_num.rs:498`（提前结束）
  文档承诺在 `core/src/json_num.rs:358`

- **问题简述**：跳过一行块内容后 `continue` 时 `i` 停在**该行的 `\n` 上**；下一轮循环把 `\n` 判为「空行」，于是 `block_indent` 被清空，块标量提前结束，后续内容行全部进入普通扫描。

- **证据**

```rust
// core/src/json_num.rs:486-499
let blank = line_start + ws >= bytes.len()
    || bytes[line_start + ws] == b'\n'
    || bytes[line_start + ws] == b'\r';
if !blank && ws >= indent {
    while i < bytes.len() && bytes[i] != b'\n' {   // ← 停在 '\n' 上
        i += 1;
    }
    continue;                                       // ← 未越过 '\n'
}
// 缩进不足（或空行）→ 块标量结束。
block_indent = None;
```

推演（`"n: |\n  1e-400\n  2e-500\n"`）：
1. 命中 `|`，`block_indent = 2`，跳过块头行并越过 `\n`（第 524-529 行处理正确）。
2. 第 1 轮：`i` 在 `  1e-400` 行首，`ws=2 >= 2`，`!blank` → 跳到行尾 `\n`，`continue`。
3. 第 2 轮：`line_start = i` 指向 `\n` → `ws = 0` → `blank = true` → **不清空就怪了**，`block_indent = None`（第 498 行）。
4. 第 3 轮起：`2e-500` 按普通标量扫描 → **成为候选** → 若确认环节判下溢则**整份文档被拒**。

- **影响**
  - 与模块文档 `core/src/json_num.rs:358`「块标量 `|` / `>` 之后的缩进行（多行字符串内容）**整体跳过**」的承诺不符。
  - 后果方向是「误拒」（fail-safe），不会产出错误 G-code；但会让**合法的**清单 / 变量库 / 机床预设文件被拒绝加载，用户只能删内容绕过。行数越多，漏网内容越多。

- **改进建议**（改写 486-499 段）：

```rust
// 空行属于块标量内容（YAML 中空行是内容的一部分）：越过它并保持 block_indent
let blank = line_start + ws >= bytes.len()
    || bytes[line_start + ws] == b'\n'
    || bytes[line_start + ws] == b'\r';
if blank {
    while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
    if i < bytes.len() { i += 1; }          // 越过行尾，防止空行死循环
    continue;
}
if ws >= indent {
    while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
    if i < bytes.len() { i += 1; }          // ★ 越过行尾，与块头处理（524-529）同构
    continue;
}
block_indent = None;                        // 缩进不足 → 块标量正常结束
```

- **应补充的测试用例**

```rust
#[test] fn yaml_block_scalar_skips_all_content_lines() {
    assert_eq!(hits_yaml("n: |\n  1e-400\n  2e-500\n"), 0);   // 当前实现得 1（假阳性）
    assert_eq!(hits_yaml("n: |\n  1e-400\n\n  2e-500\n"), 0); // 含空行
    assert_eq!(hits_yaml("n: >\n  1e-400\n  2e-500\nm: 1e-500\n"), 1); // 块外仍命中
}
```

---

#### Q-04　YAML 的 `#` 未按「前需空白/行首」判定 → 标量内的 `#` 造成**漏报**

- **文件:行号**：`core/src/json_num.rs:504`

- **问题简述**：`Normal` 态下**任意位置**的 `#` 都被当作注释起点。YAML 规范要求注释的 `#` 必须位于行首或其前有空白；标量内部（如 URL 片段 `http://a#b`）的 `#` 不是注释。

- **证据**

```rust
// core/src/json_num.rs:504
if mode == LexMode::Yaml && b == b'#' {
    state = ScanState::InComment;   // 未检查前一个字节是否为空白/行首
```

推演：`"u: http://a#1e-400"` → 在 `#` 处进入 `InComment` 直到行尾 → `1e-400` 被吞 → **命中 0**。若该 YAML 是变量库/清单，一个真下溢字面量就静默漏网（随后被 `serde_yaml` 归零写进 G-code）。

- **影响**：与本项目「预筛**不可能漏掉真下溢值**」（`core/src/json_num.rs:14-16`）的承诺直接冲突。实际触发概率低（需要标量内含 `#` 且其后紧跟下溢字面量），但属**零容忍域的漏报通道**，必须封堵。

- **改进建议**

```rust
let prev_is_ws = i == 0 || matches!(bytes[i - 1], b' ' | b'\t' | b'\n' | b'\r');
if mode == LexMode::Yaml && b == b'#' && prev_is_ws {
    state = ScanState::InComment;
    i += 1;
} else if ...
```

- **应补充的测试用例**：`yaml_hash_inside_plain_scalar_is_not_comment`：`hits_yaml("u: http://a#1e-400") == 1`（当前为 0）。

---

#### Q-05　区间/整数约束经 `as_f64()` 走 `i64 as f64` 强转；白名单用 f64 `==` 跨类型比较

- **文件:行号**
  - `core/src/validate.rs:914`（`let Some(n) = value.as_f64() else { return; }`）
  - `core/src/model.rs:274`（`ParamValue::Integer(v) => Some(*v as f64)`）
  - `core/src/model.rs:332-333`（`matches_option` 中 `*a == *b as f64`）

- **问题简述**：`as_f64()` 对 `Integer` 做无条件 `as f64`；超过 2^53 的整数不再可精确表示，`min` / `max` 判定会失真。`matches_option` 用 f64 `==` 判白名单成员，跨 `Integer`/`Number` 时同样受此影响。

- **证据**

```rust
// core/src/model.rs:271-277
pub fn as_f64(&self) -> Option<f64> {
    match self {
        ParamValue::Number(v) => Some(*v),
        ParamValue::Integer(v) => Some(*v as f64),   // ← >2^53 静默丢精度
        _ => None,
    }
}
```

- **影响**：CNC 实际量级（程序号 ≤ 99999、转速 ≤ 6000、行号 ≤ 9999）**无现实风险**，属潜在缺陷。但校验层是通用设施：`line_number_digits` 之类被「夹紧在 `[1,32]`」的参数一旦放宽上限，或自定义机床配了超大整数量，就会出现「值实际越界但比较通过」。

- **改进建议**

```rust
/// 精确可表示的 f64 视图：超出 2^53 的整数返回 None，由调用方报
/// 「超出可精确比较范围」而非静默用近似值判定。
pub fn as_exact_f64(&self) -> Option<f64> {
    match self {
        ParamValue::Number(v) => Some(*v),
        ParamValue::Integer(v) if v.unsigned_abs() <= (1u64 << 53) => Some(*v as f64),
        _ => None,
    }
}
```
`check_value_constraints` 改用 `as_exact_f64()`，并在返回 `None` 且 `spec.min/max/integer` 有声明时产出一条 `IssueKind::Other` 警告（说明无法精确判定）。

- **应补充的测试用例**：`integer_beyond_f53_precision_is_reported_not_silently_compared`（`ParamValue::Integer(i64::MAX)` 对 `with_max(99999.0)`）。

---

#### Q-06　对外 JSON 契约不含 `kind` 字段，`IssueKind` 的设计目的在两条通道上落空

- **文件:行号**：`core/src/validate.rs:1104-1111`（结构体定义）、`core/src/validate.rs:1121-1131`（`json_view`）、`core/src/validate.rs:49-99`（`IssueKind` 的设计说明）

- **问题简述**：`ValidationIssueJson` 只序列化 `level` / `param` / `message`，**不含 `kind`**。而 `IssueKind` 存在的唯一理由（同文件 52-54 行自述）就是「调用方需要**按类别**而非按消息文本做决策……靠 `message.contains("NaN")` 这样的文本匹配是脆弱且易失效的」。

- **证据**

```rust
// core/src/validate.rs:1102-1111
pub struct ValidationIssueJson<'a> {
    pub level: &'static str,
    pub param: Option<&'a str>,
    pub message: &'a str,          // ← 没有 kind
}
```

- **影响**：HTTP（`cli/src/server.rs`）与 CLI（`--format json`）两条通道的调用方（Web UI）都拿不到 `NonFinite` 类别，无法把「NaN/Inf 硬失败」与「缺参」区分展示，只能退回被本模块明令禁止的 `message.contains(...)`。海外/前端同学改一次文案就会静默破坏前端分支。

- **改进建议**

```rust
impl IssueKind {
    /// 机器可读类别（`snake_case`，对外契约字段，改名即破坏协议）。
    pub fn as_str(&self) -> &'static str { /* non_finite / derive_failed / ... */ }
}
pub struct ValidationIssueJson<'a> {
    pub level: &'static str,
    pub kind: &'static str,        // ★ 新增
    pub param: Option<&'a str>,
    pub message: &'a str,
}
```
并在 `json_view_freezes_contract_fields`（`core/src/validate.rs:1168`）中增加 `kind` 的冻结断言。

- **应补充的测试用例**：`json_view_exposes_issue_kind_for_programmatic_decisions`。

---

#### Q-07　`part` 聚合错误只回传每道工序的**第一条**校验问题

- **文件:行号**：`core/src/part.rs:429-436`（`PipelineError::Validation` 分支取 `issues.first()`）、`core/src/part.rs:196-203`（`OpFailure` 结构）

- **问题简述**：`describe_pipeline_error` 对校验失败只取 `report.issues.first()` 一条塞进单行 `error`。

- **证据**

```rust
// core/src/part.rs:429-436
PipelineError::Validation(report) => {
    let first = report
        .issues
        .first()                      // ← 只取第一条
        .map(|i| i.message.clone())
        .unwrap_or_else(|| "参数校验未通过".to_string());
    format!("参数校验未通过：{first}")
}
```

- **影响**：与 `PartError::OperationsFailed` 的设计初衷（"一次报全，而不是修一个跑一次又冒一个"，`core/src/part.rs:178-180`、:922）**自相矛盾**：跨工序做到了全报，**单工序内**却只报 1 条。一道工序缺 3 个参数时用户要跑 3 轮。

- **改进建议**

```rust
PipelineError::Validation(report) => {
    const MAX_SHOWN: usize = 3;
    let mut msg = format!("参数校验未通过（共 {} 条）：", report.issues.len());
    for (i, iss) in report.issues.iter().take(MAX_SHOWN).enumerate() {
        if i > 0 { msg.push_str("；"); }
        msg.push_str(&iss.message);
    }
    if report.issues.len() > MAX_SHOWN {
        msg.push_str(&format!("；…等共 {} 条", report.issues.len()));
    }
    msg
}
```
（保持单行，避免冲散 `PartError` 的聚合列表；完整报告由调用方另行展示。）

- **应补充的测试用例**：`op_failure_lists_more_than_one_validation_issue`（单工序缺 3 个必选参数 → 聚合错误里至少出现 2 个参数名）。

---

### 2.3 P2-一般

#### Q-08　`spec_map` 用 `HashMap::collect()`，同名规格静默「后者胜」

- **文件:行号**：`core/src/validate.rs:393-394`（`check_vars`）、`core/src/validate.rs:702-703`（`check_param_values`）
- **证据**：`specs.iter().map(|s| (s.name.as_str(), s)).collect()` —— `collect` 对重复 key 保留最后一个，无告警。
- **影响**：规格数组里出现同名条目（清单合并、变量库 `apply` 与头部声明叠加时理论上可构造）时，前一条的 `min/max/options` **静默失效**，与本模块自己定义的 `SpecInert`（"配置侧的静默失效"）同类，却没有被检出。
- **改进建议**：构造时检测重复并产出 `IssueKind::SpecInert` 警告（消息含参数名）。
- **补充用例**：`duplicate_spec_name_is_reported_not_silently_deduped`。

---

#### Q-09　有限性检查的递归深度上限 32 超限后**静默 return**

- **文件:行号**：`core/src/validate.rs:762-765`
- **证据**：`const MAX_DEPTH: usize = 32; if depth > MAX_DEPTH { return; }`
- **影响**：深度 >32 的嵌套列表（或 YAML 锚点构造的自引用结构）中的 NaN 会被**静默放过**，与「宽松模式唯一硬失败项 = NonFinite」的承诺（`core/src/validate.rs:102-113`）有缝隙。注释已说明是「宁可放过极端情况」的取舍，但**没有任何告警**，属于"静默"而非"显式降级"。
- **改进建议**：超限时产出一条 `IssueKind::Other` 的 Info/Warning（"嵌套过深，第 N 层以下未检查有限性"），让"放过"变成可见行为。
- **补充用例**：`finiteness_depth_cap_is_reported_not_silent`。

---

#### Q-10　`line_col` 每个候选做一次 O(n) 前缀扫描

- **文件:行号**：`core/src/json_num.rs:648-655`
- **证据**：`let prefix = &text[..offset]; prefix.bytes().filter(...).count()` + `text[line_start..offset].chars().count()`
- **影响**：候选数 m、文本长 n 时复杂度 O(m·n)。正常工艺文件（候选 0~2 个）无影响；若有人构造含数千下溢字面量的恶意/损坏文件，扫描会退化。属**健壮性**而非正确性问题。
- **改进建议**：扫描时顺带维护 `line` / `line_start` 游标（单次 O(n)），或在候选数超过阈值（如 64）时降级为只报 `byte_offset`。
- **补充用例**：`scan_1mb_json_is_bounded`（release + `#[ignore]`，与 `core/tests/large_program.rs:151` 同风格）。

---

#### Q-11　`nc_strip` 无数量级上限，与模块文档承诺不符

- **文件:行号**：`src/filters.rs:113-124`（实现）、`src/filters.rs:3-4`（文档承诺）
- **证据**

```rust
// src/filters.rs:3-4
//! 所有过滤器对结果做有限性校验（NaN/Inf 一律转渲染错误）；NC 过滤器
//! 附带数量级上限防护（防巨量分配与饱和截断）。     // ← nc_strip 没有
// src/filters.rs:113-124：只有 is_finite() 检查与 -0.0 归一，无上限
```
`nc_fixed` / `nc_signed` 有 `MAX_NC_FIXED_DECIMALS = 32`（:24），`nc_pad` 有 `MAX_NC_PAD_WIDTH = 1024`（:26），唯独 `nc_strip` 裸奔。
- **影响**：`{{ x | nc_strip }}` 在 `x = 1e300` 时输出 300+ 位数字串（Rust `Display` 不写科学计数法），直接拼进 G-code 坐标字；`x = 0.1+0.2` 输出 `0.30000000000000004`（17 位有效数字）。两者都不报错。控制器对超长数字字通常报 `Format error` 或截断 —— 后者即静默错误。
- **改进建议**

```rust
const MAX_NC_STRIP_CHARS: usize = 24;   // 覆盖 CNC 全部现实量级仍有余量
let s = format!("{}", value);
if s.len() > MAX_NC_STRIP_CHARS {
    return Err(minijinja::Error::new(
        minijinja::ErrorKind::InvalidOperation,
        format!("nc_strip: 值 {value} 的十进制展开超过 {MAX_NC_STRIP_CHARS} 字符；\
                 请改用 nc_fixed(n) 显式指定精度")));
}
Ok(s)
```
- **补充用例**：`nc_strip_rejects_astronomical_magnitude`、`nc_strip_rejects_float_noise_tail`。

---

#### Q-12　`validate_config_keys` 对 `Integer` 键只验「能否解析」，不验取值范围

- **文件:行号**：`core/src/machine.rs:347-353`
- **证据**

```rust
MachineKeyKind::Integer => {
    if v.parse::<i64>().is_err() {          // 只判"是不是整数"
        warnings.push(...);
    }
}
```
`max_spindle_rpm = "0"` / `"-100"` / `"999999999"` 全部通过（仅 `"abc"` 报警）。
- **影响**：当前 `max_spindle_rpm` 不参与任何校验，故无现实后果；**但一旦按 Q-01 接上联动**，一个 `0` 会让所有转速判为越界（全量误杀），一个负数是更糟的静默错误。这是 Q-01 落地前的**前置依赖**。
- **改进建议**：`Integer` 键解析成功后继续做范围校验（`max_spindle_rpm` / `program_digits` / `line_number_digits` 必须 `> 0`，且 `max_spindle_rpm` 建议 ≤ 100000），非法时报 warning 并提示回退默认值。
- **补充用例**：`zero_or_negative_max_spindle_rpm_warns`。

---

#### Q-13　派生链的源参数拿不到变量库的类型/区间约束

- **文件:行号**：`core/src/variables.rs:198-215`（`apply` 只对"模板引用到的名字"注入）、`core/src/derive.rs:229-231`（注释自述该缺口）
- **证据**

```rust
// core/src/variables.rs:208-213
let relevant: Vec<ParamOverride> = self.entries.iter()
    .filter(|e| names.contains(&e.name))   // ← names 来自模板引用 + 头部声明
    .cloned().collect();
```
`tip_model` 只作为派生源存在、不出现在模板正文时，拿不到库里给它声明的 `options` 白名单。
- **影响**：已被 `DeriveError::NoMatch`（`core/src/derive.rs:236`）兜住 —— 拼错型号会硬失败，不会静默产出错误深度。**但类型 / 区间 / 单位约束仍然缺失**，若源参数是数值型（如某尺寸档位），非法档位只在查表失败时才暴露，路径更长、报错更不直观。
- **改进建议**：`VariableLibrary::apply` 的 `names` 集合并入「所有 `derive.from` 指向的参数名」（需在清单层把 derive 规则一并传给 `apply`，或在 `apply` 签名增加 `extra_names: &[String]`）。
- **补充用例**：`variable_library_covers_derive_source_params`。

---

#### Q-14　`part` 层无法覆盖 `line_number_start` / `line_number_step`，超长零件静默断号

- **文件:行号**：`core/src/part.rs:104-119`（`PartOpOptions` 只 4 个开关）、`core/src/part.rs:273-301`（`op_options`）、`core/src/part.rs:1094-1122`（现有告警测试）
- **影响**：跨工序续编撞到 pipeline 的上限（默认 9999）后，后续**所有**工序都不再编号，仅靠 `warnings` 提示（已验证 `core/src/part.rs:1105-1115`）。但 `PartOpOptions` 无法调整 `line_number_start` / `step`，用户无法把行号步长改小来容纳更长程序。
- **改进建议**：`PartOpOptions` 增 `line_number_start: Option<u32>` 与 `line_number_step: Option<u32>`（带 `deny_unknown_fields`，与 P2-3 一致），并在 `op_options` 中透传。
- **补充用例**：`line_number_cap_warning_covers_every_affected_op`（3 道工序、第 2 道起全部撞限 → 告警应覆盖工序 2 与工序 3，当前实现只报 1 条）。

---

#### Q-15　资产名未拒绝 Windows 保留设备名 / 尾随点空格 / 超长路径

- **文件:行号**：`core/src/asset/path.rs:15-40`（`validate_asset_name`）
- **证据**：现有拒绝项为空串 / 控制字符 / `.` / `..` / `\` / 多段 / 绝对 / 盘符 / UNC（:16-39），**无** `CON` / `PRN` / `AUX` / `NUL` / `COM1-9` / `LPT1-9`，也无尾随 `.` / 空格，也无长度上限。测试 `core/src/asset/path.rs:118-139` 同样未覆盖。
- **影响**：Windows 上 `nctool templates add CON` 会让后续写盘操作落到控制台设备而非文件（行为取决于 Win32 层，可能静默"成功"但不产生文件，或直接报错）；`foo.` / `foo ` 会被 Win32 静默裁剪成 `foo`，造成"我明明存了 `foo.` 却在清单里找不到"。属低概率但**零成本可封堵**的边界。
- **改进建议**

```rust
// 去扩展名后的主干（按第一个 '.' 切分）大写命中即拒 —— CON.NC 同样要拒
#[cfg(windows)]
const RESERVED_STEMS: &[&str] = &["CON","PRN","AUX","NUL",
    "COM1","COM2",...,"COM9","LPT1",...,"LPT9"];
// 尾随 '.' / ' '：Windows 会静默裁剪 —— 明确拒绝而不是让用户困惑
if name.ends_with('.') || name.ends_with(' ') { return Err(...); }
// 长度：单段名上限（如 128 字符），避免叠加路径后撞 MAX_PATH
```
- **补充用例**：`reserved_windows_device_names_rejected`（`#[cfg(windows)]`）、`trailing_dot_or_space_rejected`。

---

## 3. TOP 3 —— 必须优先修复

| 序 | 问题 | 位置 | 为什么排第一梯队 |
|---|---|---|---|
| **1** | **Q-01** 主轴转速上界 6000 硬编码、与 `machine.max_spindle_rpm` 不联动 | `core/src/registry.rs:951`、`core/src/machine.rs:76/96` | **唯一一条可导致机床事故的现实通路**：WFL（3500）下 `S5000` 校验全绿并直接写进 G-code。后果是主轴超速、刀具破损、工件飞出。修复工作量可控（`ParamSpec` 加 1 字段 + 校验层透传 `MachineConfig`），且能一次性带动 Q-02、Q-12 一起收敛。 |
| **2** | **Q-02** 全部 RPM / 进给参数连静态上界都没有 | `templates/variables.yaml:100/106/127/177`、`templates/templates.yaml`（无覆盖） | 与 Q-01 同一风险域但口子更宽：12 个模板引用的转速参数**完全裸奔**，`S999999` 全绿。改动只是给变量库补 `min`/`max` 字段 + 加一条守卫测试，**零代码改动、当天可落地**。 |
| **3** | **Q-03 + Q-04** `json_num` 的 YAML 词法两处边界缺陷（块标量只跳首行 / `#` 位置判定） | `core/src/json_num.rs:490/498`、`core/src/json_num.rs:504` | Q-04 是**零容忍域的漏报通道**（与"不可能漏掉真下溢值"的承诺直接冲突，漏报 = 错误坐标进 G-code）；Q-03 是假阳性误拒 + 文档承诺不符。两处都是 5~10 行的词法修正，附带 3 条测试即可关闭。 |

> 若只能做一件事：**先做 Q-02**（0 代码改动、当天见效、堵住最宽的口子），再排 Q-01（需改签名，跨 crate）。

---

## 4. 测试覆盖率缺口清单

### 4.1 总体基线

- 生产行覆盖 **92.55%**（8198/8858，`cov.log`）；本次 6 个重点源码文件均**未进入**「覆盖率最低的 10 个文件」，即 ≥ 90%。
- `#[ignore]` 全仓 **2 处**，均已注明跑法、均非正确性测试：
  - `core/tests/large_program.rs:151`（`#[ignore = "万行级实测…"]`）—— 性能
  - `cli/src/context.rs:853`（`spike_registry_build_cost`）—— 探针/决策记录
  - **结论：不存在被静默跳过的正确性测试。**
- 快照框架：**未使用**（无 `insta` 依赖、无 `.snap` 文件），期望值均为内联字面量 —— 无「基线自动接受」漂移风险。

### 4.2 缺口清单

| # | 模块 / 入口 | 现有测试 | 缺失场景 | 建议补充用例（名称 → 断言要点） |
|---|---|---|---|---|
| 1 | `json_num` YAML 块标量 | `json_num.rs:1073`（仅单行） | 多行块标量第 2 行起、块内空行、`\r\n` 行尾、`\|2` 缩进指示符 | `yaml_block_scalar_skips_all_content_lines` → `"n: \|\n  1e-400\n  2e-500\n"` 命中 **0**（当前 1） |
| 2 | `json_num` YAML `#` 位置 | `json_num.rs:1085-1095`（整行/行尾注释） | 标量内部 `#`（URL 片段） | `yaml_hash_inside_plain_scalar_is_not_comment` → `"u: http://a#1e-400"` 命中 **1**（当前 0） |
| 3 | **JSON 侧下溢确认** `cli/src/output.rs:177` | `cli/src/output.rs:449-490`（4 条：基本命中 / 字符串放过 / CLI 前缀 / 合法值） | **阈值分裂关键项 `2.4703282292062328e-324` 在 JSON 侧无测试**（YAML 侧已在 `json_num.rs:1115` 断言「不归零」） | `confirm_json_underflow_flags_2470e_324` → JSON 侧必须 `true`；与 YAML 侧 `false` 形成**对照对**，钉死"两个解析器分别确认"的设计 |
| 4 | `json_num` 负号下溢确认 | `json_num.rs:1109-1113`（`1e-400` / `1e-324`） | `-1e-400`（解析为 `-0.0`，`== 0.0` 路径） | `confirm_underflow_yaml_accepts_negative` → `-1e-400` 必须 `true` |
| 5 | `json_num::line_col` | `json_num.rs:1019-1026`（纯 ASCII） | 数字前有中文 / emoji 时的列号与 `byte_offset` | `column_is_char_based_with_multibyte_prefix` → `column` 按字符计、`byte_offset` 按字节计，两者不一致但都正确 |
| 6 | `json_num` 规模 / 性能 | 无 | 1 MB+ JSON、数千候选的扫描耗时与内存 | `scan_1mb_json_is_bounded`（release + `#[ignore]`，仿 `core/tests/large_program.rs:151`） |
| 7 | `validate` × 机床联动 | **无** | 转速 / 进给上界按机床取值；非法机床值回退 | `spindle_speed_bound_follows_machine_max_spindle_rpm`（8 组边界，见 Q-01）；`invalid_machine_max_falls_back_with_warning` |
| 8 | `validate` 大整数区间 | `validate.rs:1850`（`99999`） | `i64::MAX` / 2^53+1 对 `min`/`max` 的判定 | `integer_beyond_f53_precision_is_reported_not_silently_compared`（配合 Q-05） |
| 9 | `validate` JSON 契约 `kind` | `validate.rs:1168-1211`（冻结 `template/ok/errors/warnings/issues[].level/param/message`） | `issues[].kind` 字段 | `json_view_exposes_issue_kind_for_programmatic_decisions`（配合 Q-06） |
| 10 | `validate` 规格同名 | 无 | 规格数组含同名条目 | `duplicate_spec_name_is_reported_not_silently_deduped`（配合 Q-08） |
| 11 | `validate` 有限性深度上限 | `validate.rs:1558-1585`（嵌套 2 层 NaN） | 深度 > 32 时被静默放过 | `finiteness_depth_cap_is_reported_not_silent`（配合 Q-09） |
| 12 | `part` 聚合错误完整性 | `part.rs:924-948`（**跨**工序 2 条） | **单工序内**多条校验问题只回传 1 条 | `op_failure_lists_more_than_one_validation_issue`（配合 Q-07） |
| 13 | `part` 行号上限 | `part.rs:1095-1122`（2 工序，只第 2 道撞限） | 3+ 工序全部撞限；`line_number_start`/`step` 不可覆盖 | `line_number_cap_warning_covers_every_affected_op`；`op_options_can_override_line_number_step`（配合 Q-14） |
| 14 | `machine` 预设上限关系 | `machine.rs:491-514`（schema ↔ generic 逐键相等） | WFL=3500 / INDEX=5000 与 generic=6000 的**大小关系**无断言 | `preset_max_spindle_rpm_is_at_most_generic` → 防止有人把 3500 改回 6000 而无测试发现（Q-01 的防回归闸门） |
| 15 | `machine` 配置值范围 | `machine.rs:571-579`（`abc` / `imperialish`） | `max_spindle_rpm = 0 / -100 / 999999999` | `zero_or_negative_max_spindle_rpm_warns`（配合 Q-12） |
| 16 | `variables` 变量库鲁棒性 | `variables.rs:239-481`（13 条） | BOM、CRLF、`options` 重复值、超长文档、`derive` 经库覆盖 | `variables_with_bom_and_crlf_parse`；`duplicate_option_values_rejected` |
| 17 | `variables` 派生源覆盖 | `variables.rs:387-408`（引用但未声明） | 派生源参数（`tip_model`）不在模板正文时拿不到库约束 | `variable_library_covers_derive_source_params`（配合 Q-13） |
| 18 | `derive` 字符串查表键 | `derive.rs:422-434`（**数值**键跨 Integer/Number） | 字符串键大小写敏感（`"b4"` vs `"B4"`）、空 `table` + `fallback` + 源未提供 | `string_table_key_is_case_sensitive` → `"b4"` 必须 `NoMatch`；`empty_table_with_fallback_and_no_source` |
| 19 | `asset` 原子写 + 路径 | `core/tests/asset_write.rs`（约 32 断言点）+ `asset_adversarial.rs`（约 48）覆盖：重命名替换 / 非空目录 / 只读 / 父目录缺失 / 重试退避 / 无临时文件残留 | **Windows 保留设备名（CON/NUL/COM1/LPT1）、尾随 `.` / 空格、超长路径（>260）、中文/emoji 路径、并发同路径写入、磁盘满** | `reserved_windows_device_names_rejected`（`#[cfg(windows)]`，配合 Q-15）；`trailing_dot_or_space_rejected`；`unicode_and_emoji_asset_names_round_trip`；`concurrent_writes_to_same_path_never_corrupt` |
| 20 | 端到端：机床 × 转速 | `cli/tests/cli_machine_e2e.rs`（105 断言点）、`cli_preset_e2e.rs`（54） | 无「按机床拒绝超速」的 e2e | `cli_rejects_spindle_speed_above_machine_limit`：`nctool render tool_change --machine wfl_m65 --param tool_num=1 --param spindle_speed=5000` 必须非 0 退出且 stderr 含 `3500` |

### 4.3 断言强度抽查（是否有"只 assert 不 panic"的弱断言）

- 本次抽查未发现「空 `assert!(true)`」类假断言。
- 发现 **1 处**值得注意的**自我实现的断言**：`core/src/part.rs:1176-1201` 的注释已诚实指出「内建三预设的 `line_number_prefix` 当前都是 `N`，故无法靠前缀区分机床，本用例改以游标推进作为证据」——**方向正确**，建议把这段"为什么不用文本断言"的理由保留在代码里（已保留），勿被后人改回文本断言。
- `core/src/validate.rs:1278-1285` `validate_with_vars_agrees` 只断言 `r1.issues.len() == r2.issues.len()`（**只比数量，不比内容**）——两条路径可能产生同数量但不同类别的问题。建议加强为逐条比对 `(kind, level, param)`。

---

## 5. 已确认的良好实践（无需改动，勿被后续重构拆掉）

1. **`json_num` 的「文本层预筛 + 实际解析器确认」两段式**（`core/src/json_num.rs:1-47`）：显式承认 `serde_json` 非正确舍入、`serde_yaml` 正确舍入的**阈值分裂**，并因此拒绝"单一预测阈值"方案。这是本项目数值处理上最扎实的一处设计，Q-04 的修复不得引入任何"预测阈值"。
2. **`IssueKind::is_hard_fail` 的穷尽匹配**（`core/src/validate.rs:110-132`）：新增类别时编译器强制表态，避免白名单反向导致的"新类别被默认降级"。配套守卫测试 `downgrade_soft_errors_keeps_hard_fail_only`（:1681）钉得很牢。
3. **`nc_fixed` / `nc_signed` 的「字符串判零」判据**（`src/filters.rs:85-91`、:164-169）：注释里逐条列出"为什么不能用 `0.5×10^-N` 量级式判据"（含 N=0..8 的实测结论），并明确 tie 点行为"正确且有意保留，勿修"。负零归一在**舍入之后**做（:80-86），避开了 `-1e-4 → -0.000` 的坑。
4. **`derive` 的 `fallback` 语义收窄**（`core/src/derive.rs:191-198`、`docs/CODE_REVIEW_2026-09-23.md` P1-2）：「未提供 → fallback」与「提供了但未命中 → 一定报错」分开，把"型号拼错 → 按 DM24 处理"这种静默错误堵住了。
5. **`part` 的事务语义**（`core/src/part.rs:407-411`）：任一工序失败**整体不交付**，且 `PartError` 里没有 `outcome` 字段——用类型系统保证调用方拿不到半成品。
6. **`asset/path.rs` 的 D14 双层校验**（`core/src/asset/path.rs:32-39`、:86-99）：不仅校验名称形态，还在**结果路径**上再断言一次落在安全根内，识破 `Z:` 这类被 `Path::components()` 放行的前缀替换逃逸。
7. **原子写的 Windows 杀毒扫描重试**（`core/src/asset/atomic.rs:51-70`）：只对 `PermissionDenied` 重试、5 次上限、延迟可注入 —— 单测能确定性地覆盖重试各分支而不依赖真实 Defender。
8. **`part.rs:463-472` 的测试隔离**：临时目录名用原子自增序号而非 `files.len()`，避免了「所有 2 模板用例共用目录互相覆盖」的并行污染。

---

## 附录 A：本次审查执行的关键只读命令（可复现）

```bash
# 1. 确认 6000 硬编码点与机床预设值
grep -rn "6000" core/src/ cli/src/ templates/
grep -rn "max_spindle_rpm" core/src/registry.rs templates/     # → 0 命中（关键证据）

# 2. 确认下溢扫描的调用方覆盖度
grep -rn "scan_underflow_candidates\|confirm_underflow" --include=*.rs .
# → core: variables.rs:81, manifest.rs:420, asset/preset.rs:584
# → cli : output.rs:162（--params-file / --param / part 文件 / HTTP 请求体）
#   结论：四条入口（params-file、--param、part 文件、HTTP body）+ YAML 侧三处，均已覆盖

# 3. 确认 #[ignore] 全貌
grep -rn "^\s*#\[ignore\]" --include=*.rs .      # → 仅 2 处，均性能/探针类

# 4. 覆盖率基线
head -40 cov.log        # 生产行覆盖 8198/8858 = 92.55%
```

## 附录 B：问题索引

| 编号 | 严重度 | 位置 | 一句话 |
|---|---|---|---|
| Q-01 | **P0** | `core/src/registry.rs:951` | 转速上界 6000 硬编码，机床 `max_spindle_rpm` 不联动 |
| Q-02 | P1 | `templates/variables.yaml:100/106/127/177` | RPM/进给参数完全无上界 |
| Q-03 | P1 | `core/src/json_num.rs:490/498` | YAML 块标量只跳过首行 |
| Q-04 | P1 | `core/src/json_num.rs:504` | YAML `#` 未按空白/行首判定 → 漏报 |
| Q-05 | P1 | `core/src/model.rs:274` | `i64 as f64` 强转 >2^53 精度丢失 |
| Q-06 | P1 | `core/src/validate.rs:1104-1111` | JSON 契约缺 `kind` 字段 |
| Q-07 | P1 | `core/src/part.rs:429-436` | 聚合错误只回传首条 |
| Q-08 | P2 | `core/src/validate.rs:393-394` | 同名规格静默后者胜 |
| Q-09 | P2 | `core/src/validate.rs:762-765` | 深度上限超限静默放过 |
| Q-10 | P2 | `core/src/json_num.rs:648-655` | `line_col` O(m·n) |
| Q-11 | P2 | `src/filters.rs:113-124` | `nc_strip` 无数量级上限 |
| Q-12 | P2 | `core/src/machine.rs:347-353` | Integer 配置键不验范围 |
| Q-13 | P2 | `core/src/variables.rs:208-213` | 派生源参数拿不到库约束 |
| Q-14 | P2 | `core/src/part.rs:104-119` | 无法覆盖 `line_number_start`/`step` |
| Q-15 | P2 | `core/src/asset/path.rs:15-40` | 未拒 Windows 保留名/尾随点空格 |
