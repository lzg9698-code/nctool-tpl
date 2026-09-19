# nctool 代码审查报告（第三轮）

> 审查日期：2026-09-18
> 审查对象：`rustjinja` workspace @ `6cdfa27`（工作区干净）
> 代码规模：`nctool-tpl` 0.4.0 / `nctool-core` 0.3.0 / `nctool-cli` 0.3.0，生产 + 测试共 16 764 行 Rust
> 审查方式：全量通读三个 crate + `ui/index.html` + 测试/CI 配置；关键结论均经**实测复现**，不接受推断
> 历史对照：`CODE_REVIEW_AND_DEV_PLAN.md`（09-05，Web UI 首轮）、`docs/ARCHITECTURE_REVIEW.md`（09-15，架构评估）

---

## 0. 本轮做了什么验证（结论的可信度依据）

| 手段 | 结果 |
| --- | --- |
| `cargo clippy --workspace --all-targets` | **零告警**，通过 |
| `cargo test --workspace --all-targets` | 通过（基线） |
| 解析 `lcov.info`（09-18 11:57 生成）独立计算覆盖率口径 | 复现「生产口径 88.40%」结论 |
| 独立 `rustc` 程序实测浮点格式化/舍入/`as i64` 饱和行为 | 复现 4 条数值类缺陷 |
| 对照 Python 3.13 `format(v,'.Nf')` | 证明舍入模式与源项目一致（排除误报） |
| 逐行核对 `ui/index.html` 插值点与 `esc()` 实现 | 确认 XSS 向量 |
| 逐行核对校验/派生/清单/路径四处控制流 | 确认 4 条静默出错路径 |

**未修改任何代码**（本次为纯审查）。

---

## 1. 结论摘要

| 维度 | 评分 | 一句话依据 |
| --- | ---: | --- |
| 架构与分层 | **8.5** | 依赖严格单向、`core` 无 I/O、单一来源原则落地；`--param` 对拍门禁是全仓最佳设计 |
| 可维护性 | **8.0** | 注释解释「为什么」、零 TODO、`check_vars` 已拆；扣分在数据表内联与 `server.rs` 混合职责 |
| 正确性（核心链路） | **6.5** | 主链路扎实，但存在 4 条**静默产出错误数值**的路径（派生链、required_if 短路、列表 NaN、行号前缀） |
| 安全性 | **7.0** | 09-05 的高危 LFI 已彻底修复且比建议更严（拒绝非回环）；扣分在 UI XSS、路径泄露、无读超时 |
| 性能 | **6.5** | 注册表缓存已加；扣分在每请求全树 `stat`、`derive` 多次全量克隆 |
| 质量度量闭环 | **5.5** | 覆盖率门禁**口径失真**（声明 90%，生产实测 88.40%）、1 个自证测试、MSRV 无 CI 验证 |
| 工程化 | **8.0** | 三平台 CI、`-D warnings`、`cargo audit`、golden 逐字节比对、属性测试 |

**总体：7.4 / 10**（较 09-15 的 7.5 基本持平；扣分集中在「度量闭环」而非架构）。

**核心判断**：这不是一个需要返工的项目。09-05 的 3 个 P0 已全部修复且质量高于要求。
本轮发现的严重问题有明确的共同特征——**它们都不报错**：派生链取错源值、`required_if` 被
`default(0)` 短路、列表里的 NaN 绕过校验、行号前缀为空导致整份程序不编号。对「G-code 驱动
机床」这个语境，这比任何崩溃都危险，也正是项目自己写在文档里的「静默出错零容忍」所针对的
目标。**建议按 §3 的批次顺序处理，批次一只需改动约 20 行。**

---

## 2. 缺陷清单

### 2.1 P0 —— 严重

---

#### P0-1｜覆盖率门禁口径失真，「90% 行覆盖」当前不成立　`指纹 COV-GATE-CALIBER`

- 位置：`.github/workflows/ci.yml:118-119`、`src/lib.rs:64`
- 证据：

```yaml
- name: Generate coverage report (line gate 90%)
  run: cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info --fail-under-lines 90
```

`cargo llvm-cov` 把 `src/*.rs` 内 `#[cfg(test)]` 段本身计入分母。实测（脚本解析 `lcov.info`）：

| 口径 | 行覆盖 |
| --- | --- |
| 门禁实际测的（含 src 内测试段） | 8868/9619 = **92.19%** |
| **排除 `#[cfg(test)]` 段（生产口径）** | 4200/4751 = **88.40%** |

失真程度最大的文件：

| 文件 | 计入行数 | 其中生产行 |
| --- | ---: | ---: |
| `src/lib.rs` | 1208 | **0**（`#[cfg(test)]` 在第 64 行，后面 1760 行全是测试） |
| `core/src/validate.rs` | 1078 | 446 |
| `core/src/manifest.rs` | 821 | 337 |
| `cli/src/server.rs` | 926 | 532 |

- 影响：门禁绿 ≠ 生产代码达 90%。CI 注释里「当前基线 91.30%」与「1.3pt 余量约等于 120 行新代码」
  的说法按生产口径都不成立（生产口径需新增约 **245 行**未覆盖代码才会跌破 90%）。更糟的是
  反方向：新增「功能 + 测试」会**推高**该指标，于是覆盖率数字会随测试增长而虚涨。
- 修复（二选一）：
  1. **推荐**：把 `src/lib.rs` 等文件的单元测试迁到 `tests/`，然后
     `cargo llvm-cov --workspace --all-features --ignore-filename-regex '(/tests/|/benches/)'`
     —— 注意 `--ignore-filename-regex` 无法排除 `src/` 内的 `cfg(test)` 段，**必须靠文件位置分离**。
  2. 保留现状但把阈值改成生产口径实测值（88.40% 取整为 88），并把 CI 注释与
     `docs/CONTRIBUTING.md` 的口径说明同步改掉，不再声称 90%。

---

#### P0-2｜派生参数的链式依赖取错源值，静默产出与型号不符的数值　`指纹 DERIVE-CHAIN-STALE`

- 位置：`core/src/derive.rs:113`、`:117`、`:158`
- 证据：

```rust
let with_defaults = crate::model::apply_spec_defaults(specs, params);
let mut out = params.clone();
for spec in derived {
    let rule = spec.derive.as_ref().expect("已按 derive.is_some() 过滤");
    let source = with_defaults.get(&rule.from);   // ← 只从 with_defaults 取
    ...
    out.values.insert(spec.name.clone(), value);  // ← 结果写进 out，但 out 从不被读取
}
```

源值只从「用户值 + 规格默认值」取，**从不从 `out`（本轮已算出的派生值）取**。
`out` 只写不读，形同虚设。

- 触发条件：存在 `A.derive.from = B`，而 `B` 自身也是派生参数。
  - 用户未传 `B`、`B` 有 `default` → A 用 **B 的默认值**查表；
  - 用户传了 `B` → A 用 **将被派生覆盖的旧值**查表。
  两条路径都不报错，直接写进 G-code。
- 修复：按依赖拓扑排序迭代，或循环重算 `out` 直到不动点（`out` 收敛即停），
  对检测到的环返回 `DeriveError`。同时把 `out` 的读取纳入测试（当前无测试覆盖链式派生）。

---

#### P0-3｜`required_if` 被 `var.optional` 短路，互斥分支参数可静默产出 `Z0`　`指纹 VALIDATION-REQUIREDIF-SHORTCUT`

- 位置：`core/src/validate.rs:568`（判定）与 `:574`（条件判定）
- 证据：

```rust
let has_default = var.optional || spec.and_then(|s| s.default.as_ref()).is_some();
if has_default {
    continue;                       // ← 先于 required_if 判定
}
// 无兜底 → 缺参。若规格声明了条件必选，先按控制参数的取值判定：
let decision = spec.map(|s| required_if_decision(s, spec_map, params));
```

`var.optional` 由 `src/extract.rs:283` 在「全部引用都处于 `| default(...)` / `is defined`
兜底上下文」时置为 `true`。它**先于** `required_if_decision` 执行，于是模板里只要残留
`{{ FS_Z_PLUS1 | default(0) }}`，清单里声明的 `required_if` 就永不生效，且
**不产生任何 Missing / ConditionalSkipped / SpecInert 提示**。

- 触发条件：模板作者用 `| default(0)` 规避「互斥分支参数被误判必选」——
  这正是 `templates/templates.yaml:47-51` 与 `templates/turning/undercut_fs.j2:26`
  用文字明确禁止的做法。当前仓库 25 个模板均未违反，故**尚未触发**，但守卫只需一次模板编辑
  即可失效，且现有 6 个 `required_if_*` 测试全部使用「非兜底引用」的模板，覆盖不到这条路径。
- 影响：静默产出 `Z0` 坐标——项目文档点名的撞刀级场景。
- 修复：`required_if` 存在时**优先**做条件判定，命中即报 `Missing`，忽略 `var.optional`：

```rust
if spec.and_then(|s| s.required_if.as_ref()).is_some() {
    // 条件命中 → 必选（不受 optional 影响）；未命中 → ConditionalSkipped
} else if has_default {
    continue;
}
```

  并补一个回归测试：模板含 `{{ X | default(0) }}` + 清单声明 `required_if` + 条件命中 →
  必须报 `Missing`。

---

#### P0-4｜`check_finite` 不递归列表，「宽松模式唯一硬失败项」承诺失真　`指纹 VALIDATE-FINITE-TOPLEVEL-ONLY`

- 位置：`core/src/validate.rs:522-540`
- 证据：

```rust
if let crate::model::ParamValue::Number(n) = value {
    if !n.is_finite() { ... }
}
```

只匹配顶层 `Number`；`ParamValue::List` 不递归。而 `core/src/registry.rs:692` 的
`find_non_finite` **是递归的**（含 `Seq` / 对象 / 深度上限 32）。

- 触发条件：`passes = [{"z": NaN}]`（项目明确支持列表参数，模板写
  `{% for p in passes %}{{ p.z }}`）。
- 影响链：
  1. `validate` 不报 `NonFinite`；
  2. `generate_lenient` 的 `has_kind(NonFinite)` 判定通过，`is_ok() == true`；
  3. 随后 `ensure_finite_context`（`registry.rs:675`）递归命中，以
     `PipelineError::Render` 失败。
  结果是文档承诺的「宽松模式唯一硬失败项 = NonFinite」在**报告层与实际行为层不一致**，
  用户看到「校验通过」却拿不到程序，且错误类型从 Validation 变成 Render。
- 修复：`check_finite` 递归 `List`（复用 `find_non_finite` 的遍历逻辑即可）。

---

### 2.2 P1 —— 重要

#### P1-1｜UI 存在存储型 XSS，且 CSP 明确放行 inline 事件处理器　`指纹 WEBUI-XSS-001`

- 位置：`ui/index.html:1680-1682`（`esc` 实现）、`:1707-1708`、`:1711`、`:1871`、`:1875`、`:1891`、
  `:1893`、`:2279`；`cli/src/server.rs:49-51`（CSP）
- 证据：

```js
function esc(s) {
  return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}
```

**不转义引号**，且以下插值点连 `esc()` 都没调用：

```js
return `<div class="tpl-item ${state.selected === t.name ? "active" : ""}" data-tpl="${t.name}">
    <div class="row1"><span class="name">${t.name}</span>
```

`t.name` 对目录模板就是**磁盘文件名**（`cli/src/context.rs:205-215`）。文件名可含 `<` 与 `"`，
例如放入 `a<img src=x/onerror=alert(1)>.j2` → 打开 `nctool ui` 即执行脚本；
`data-tpl="${t.name}"` 还可被 `"` 打破属性边界注入事件处理器。
CSP 为 `script-src 'self' 'unsafe-inline'`，**不构成缓解**（`unsafe-inline` 放行属性事件处理器）。

同类插值点：`:1871`/`:1875`/`:1880`/`:1891`/`:1893` 的 `${s.name}`（参数名来自模板头部
`{# PARAMS: #}`，按空白分词、不校验字符集）；`:2279` 的机床 `${m.vendor} ${m.model}`（来自用户 TOML）；
`:1884` 的 `value="${has ? esc(cur) : ""}"`（参数值含 `"` 可逃出属性）。

- 修复：`esc()` 补齐 `"` `'` `` ` ``，并区分属性/文本上下文；上述所有插值点一律先 `esc()`。
  同时建议把 CSP 收紧为不含 `'unsafe-inline'`（需要把 inline 脚本外置或加 hash）。

---

#### P1-2｜符号链接目录导致无限递归 → 栈溢出 abort，整个服务死亡　`指纹 CTX-SYMLINK-LOOP`

- 位置：`cli/src/context.rs:363-366`
- 证据：

```rust
if path.is_dir() {
    collect_templates(root, &path, out)?;
    continue;
}
```

`Path::is_dir()` **跟随符号链接**，而逃逸校验（`:380-390` 的 `canonicalize` + `starts_with(root)`）
只对**文件**执行。目录层没有 visited 集合、没有深度上限。模块注释 `:90` 声称
「符号链接逃逸路径一律跳过」，与实际不符。对比 `tree_stamp`（`:308-328`）用
`DirEntry::metadata`（不跟随链接）故安全。

- 触发条件：模板目录内建一个指向上级的符号链接 / Windows junction 形成环
  （如 `ln -s .. t/a/loop`）。
- 影响：`nctool templates list` 或 `nctool ui` 后**任意一个 API 请求**（每次 `build_registry`
  都会走这里）都会白走这棵环形树。
  > **2026-09-18 实测更正**：本条原记为「栈溢出 → 进程 abort」，实测**不成立** ——
  > 路径每层增长一段，`is_dir()` 最终在平台路径长度上限处失败并返回 `false`，该层被
  > 当作普通文件跳过，递归因此有界（Windows 用 junction 造环实测：66 层后正常结束，
  > 无崩溃、无报错）。真实代价是每次构建注册表白走几十（Linux 更高）层目录，
  > Web UI 下每个请求重走一遍。仍须显式断环：路径长度兜底是平台偶然属性，
  > Windows 启用长路径或加 `\\?\` 前缀后即失效，那时才是真正无界递归。
- 修复：递归前对目录同样 `canonicalize` + `starts_with(root)` 校验，并维护
  `HashSet<PathBuf>` 已访问集合；或直接用 `entry.file_type()?.is_symlink()` 跳过目录链接。
  ✅ 已实施（`BTreeSet<PathBuf>` + 目录层逃逸校验，见 `cli/src/context.rs::collect_templates`）。

---

#### P1-3｜server 模式下「步进」等数字选项以字符串发出，改一次即渲染失败　`指纹 WEBUI-OPTION-TYPE-001`

- 位置：`ui/index.html:1801`（写入）、`:1956`（发送）、`cli/src/server.rs:435-443`（校验）
- 证据：

```js
// ui/index.html:1801
inp.addEventListener("input", () => { state.options[inp.dataset.optNum] = inp.value; ... });
// ui/index.html:1956
const res = await API.render(t.name, state.params, state.machine, state.options);
```

`inp.value` 是字符串；`normalizeOpts`（`:1623-1634`）**只在 demo 分支被调用**，
server 模式 `API.request` 直接 `JSON.stringify(body)`（`:1508`）。后端要求
`as_u64()`（`server.rs:435`），字符串 → `None` → 400 `options.lineStep 必须是非负整数`。

- 触发条件：`nctool ui` → 选中模板 → 在「步进」框输入 20 → 预览显示「渲染失败」。
- 修复：`API.request` 前统一过 `normalizeOpts`，或 `:1801` 写入 `Number(inp.value)`。
  建议**两者都做**（前者防其他数字字段重犯）。

---

#### P1-4｜`TemplateMeta::default()` 与 serde 默认值不一致，未入清单的模板会被静默隐藏　`指纹 MANIFEST-DEFAULT-DIVERGENCE`

- 位置：`core/src/manifest.rs:49-51`（`#[derive(Default)]` + `#[serde(default = "default_true")]`）、
  `:790`（`unwrap_or_default()`）、`:800`、`:802`
- 证据：字段声明为

```rust
#[serde(default = "default_true", skip_serializing_if = "is_true")]
pub visible: bool,
#[serde(default = "default_extension", ...)]
pub output_extension: String,
```

但 `Default::default()` 给出 `visible = false`、`output_extension = ""`，与 serde 路径
（`true` / `".NC"`）不一致。清单**未提及**的模板走 `unwrap_or_default()`，拿到的是
`visible=false` + 空扩展名，而 `cli/src/context.rs:216-217` 直接采用这两个值。

- 触发条件：往 `templates/` 放一个新 `.j2` 而不加清单条目 → 用户列表里看不到它
  （`list_visible` 过滤掉），输出文件名丢扩展名。当前 25 个模板全在清单中，属**潜伏缺陷**。
- 修复：手写 `impl Default for TemplateMeta`（`visible: true`、`output_extension: ".NC"`），
  或把字段改成 `Option<bool>` / `Option<String>` 在 `resolve` 里 `unwrap_or(true / ".NC")`。
  **并加一条测试**：清单未提及的模板 → `visible == true` 且扩展名为 `.NC`。

---

#### P1-5｜规格默认值缺有限性检查，`default: .nan` 静默注入上下文　`指纹 VALIDATE-SPEC-DEFAULT-NAN`

- 位置：`core/src/validate.rs:395-418`
- 证据：`check_spec_defaults` 依次调用 `spec.kind.matches` → `check_value_options` →
  `check_value_constraints`，**唯独不调用 `check_finite`**。
- 触发条件：清单或变量库写 `default: .nan`（YAML 的 `.nan` → `visit_f64`）。
  类型检查通过、区间比较对 NaN 恒 false、白名单未声明 → 校验全绿，
  破坏 `model.rs:872` 承诺的「校验通过 ⇒ 渲染不因缺参失败」。
- 修复：在 `check_spec_defaults` 里加 `check_finite` 等价判断。

---

#### P1-6｜机床 `line_number_prefix` 为空串 → 整份程序一行都不编号，且无告警　`指纹 PIPELINE-EMPTY-PREFIX`

- 位置：`core/src/pipeline.rs:326`、`:358`；`core/src/machine.rs:302`
- 证据：

```rust
let line_prefix = machine.get("line_number_prefix").unwrap_or("N");
...
let already_numbered = trimmed.starts_with(line_prefix)
    || (line_prefix == "N" && trimmed.starts_with('n'));
```

键**存在但值为空串**时 `unwrap_or` 不生效，`trimmed.starts_with("")` 恒真 →
每一行都被判为「已有行号」→ `line_numbers: true` 下**一行都不编号且无任何告警**。
宽度有 `clamp(1, 32)` 保护（注释 `:327-329` 明确写了这条理由），前缀没有。
且 `machine.rs:302` 的 `MachineKeyKind::String => {}` **对字符串键不做任何校验**，空串能通过。

- 触发条件：机床 TOML / 配置文件里 `line_number_prefix = ""`（用户可编辑的字符串）。
- 修复：空串回退默认值（`let line_prefix = machine.get(...).filter(|p| !p.is_empty()).unwrap_or("N");`），
  并在 `validate_config_keys` 里对 `String` 类键补「非空」校验。

---

#### P1-7｜同名文件模板被静默替换为注册表模板，用户拿到另一份程序　`指纹 RENDER-NAME-COLLISION`

- 位置：`cli/src/commands/render.rs:165`（`templates.rs:112-119` 有同样语义）
- 证据：

```rust
if !fname.is_empty() && gen.registry().get(&fname).is_none() {
    // ... 注册用户指定的文件
    return Ok((Rc::new(fresh), fname, Some(path)));
}
return Ok((gen, fname, Some(path)));   // ← 命中同名时：不注册，直接渲染注册表里那个
```

- 触发条件：模板目录根部已存在 `a.j2`（注册键 `a.j2`），此时
  `nctool --template-dir templates render /tmp/other/a.j2` → 输出的是
  `templates/a.j2` 的 G-code，**用户以为渲染的是自己指定的文件**。
- 修复：命中同名时改用唯一键（如全路径）注册，或显式报错 `template_duplicate`。

---

#### P1-8｜HTTP 分类过滤缺 `grooving` / `切槽`，切槽类模板在 UI 任何分类下都不出现　`指纹 API-CATEGORY-GAP`

- 位置：`cli/src/server.rs:209-218`（`parse_category`）；`ui/index.html:1669`（`CATS`）；
  对照 `cli/src/cli.rs:146-147`（有 `Grooving`）
- 证据：`parse_category` 只有 general/milling/turning/drilling/machine 五个分支。
- 触发条件：`GET /api/templates?category=切槽` 或 `?category=grooving` → **400**；
  前端 `CATS` 也没有「切槽」，故 `templates/grooving/` 下的模板只在「全部」里出现，
  分类计数永远对不上。另 `?category=`（空值）也被当非法值返回 400。
- 修复：`parse_category` 补齐 grooving/切槽；空串视为「不筛选」；前端 `CATS` 同步
  （**两份 `index.html` 都要改**）。

---

#### P1-9｜`options.format` 类型混淆静默回退为 gcode　`指纹 API-OPTION-FORMAT-FALLBACK`

- 位置：`cli/src/server.rs:450`
- 证据：

```rust
let format = match opts.get("format").and_then(|v| v.as_str()) {
    None | Some("gcode") => OutputFormat::Gcode,
    ...
```

`{"format": 1}` / `true` / `[]` 因 `as_str()` 返回 `None` 被归入 `None` 分支，
**静默按 gcode 生成**，调用方以为选项生效。
（同函数里 `get_bool` / `get_u32` 已对类型错误严格返回 400——此处是唯一漏网，即 09-05
`OPTIONS-SILENT-FALLBACK-001` 的残留。）
- 修复：区分「键缺失」与「类型错误」——先 `opts.get("format")`，为 `Some` 时
  `as_str()` 必须成功，否则 400。

---

#### P1-10｜`validate --format json` 绕过断管道保护，可能 panic 退出码 101　`指纹 CLI-BROKENPIPE-BYPASS`

- 位置：`cli/src/commands/validate.rs:47`
- 证据：

```rust
println!("{}", serde_json::to_string_pretty(&obj).unwrap_or_default());
```

`cli/src/output.rs:126-139` 专门实现 `write_stdout_quiet` 并注释「避免 `println!` 在管道
下游提前关闭时以 panic 收场」——此处是**全仓唯一绕过点**。
- 触发条件：`nctool validate <大模板> --format json | head -1`（报告体量超过管道缓冲时
  EPIPE → panic → 退出码 101，而契约应为 1）。`[部分待验证]`：确切触发取决于报告体积与
  平台缓冲，但偏离模块自身设计意图是确定的。
- 修复：改用 `ctx.style` / `write_stdout_quiet` 路径。

---

#### P1-11｜include 闭包的规格优先级与文档相反　`指纹 REGISTRY-INCLUDE-PRECEDENCE`

- 位置：`core/src/registry.rs:524-530`
- 证据：

```rust
self.collect_include_closure(sub, vars, specs, visited);   // 先递归（深层先入表）
...
for spec in &sub.params {
    if !specs.iter().any(|s| s.name == spec.name) {
        specs.push(spec.clone());                          // 先到先得
    }
}
```

递归在自身 `params` 之前执行，而合并策略是「先访问者优先」。文档称「更接近主模板的声明优先」。
- 触发条件：`main → child → grandchild`，child 与 grandchild 对同名参数声明了不同 `options`
  → 采用 **grandchild** 的（离主模板最远）。`[待验证]`：需构造三模板用例确认，
  但控制流顺序是确定的。
- 修复：先 push 自身 `params`，再递归。

---

#### P1-12｜清单孤儿条目零检测，`params` / `visible` / `machine` 全部静默失效　`指纹 MANIFEST-ORPHAN-KEY`

- 位置：`core/src/manifest.rs`（`TemplateManifest` 未提供未命中键报告）；
  全仓 `grep TemplateManifest::iter` 无命中
- 触发条件：清单键写成 `turning/undercut.j2`（实际为 `undercut_fs.j2`）→ 该条目的
  `params`（白名单/区间）、`visible`、`machine`、`output_extension` 全部静默失效，
  参数失去约束。
- 说明：项目已为「规格写了个不存在的参数」设了 `SpecInert`，此处却无对应机制——设计上不对称。
- 修复：加载后对未命中的键产出警告（可复用 `ResolvedMeta::warnings` 通道）。

---

#### P1-13｜稀疏覆盖无法表达「显式清空」　`指纹 MANIFEST-SPARSE-NO-CLEAR`

- 位置：`core/src/manifest.rs:211-218`
- 证据：`options: []` 被归一为 `None`（保留变量库的白名单）；
  `min` / `max` / `required_if` / `derive` / `unit` 一旦继承就**无法移除**（`Option` 字段只能设不能清）。
- 触发条件：变量库给 `U_Q` 声明 `min: 0`，某模板需要负值 → 清单里写什么都解不掉。
- 修复：用 `Option<Option<T>>` 或引入 `null` 语义区分「未写 / 清空」。

---

#### P1-14｜文档与 CI 对「覆盖率是否阻断」自相矛盾　`指纹 DOCS-COVERAGE-CONFLICT`

- 证据：
  - `docs/CONTRIBUTING.md:28`：「`cargo-llvm-cov`（覆盖率，**非阻断**）」
  - `docs/CONTRIBUTING.md:74`：「`coverage` job 同样是**阻断**项，且带阈值门」← **同一文件自相矛盾**
  - `README.md:589`：「CI 中 `coverage` job 非阻断，其余必须绿」
  - `docs/RELEASE.md:37`：「**不阻塞**：`coverage` job（`B-Backlog` 标记）非阻断」

而 `ci.yml` 已移除 `continue-on-error` 并带 `--fail-under-lines 90`。
- 影响：按文档行事的人会把红的 coverage 当噪音合并——等于从语义上把 09-15 修好的门禁又拆掉。
- 修复：三处统一改为「阻断」，删除 RELEASE.md 的 `B-Backlog` 标记。

---

#### P1-15｜退出码矩阵测试是自证测试，零保护能力　`指纹 TEST-EXITCODE-SELF-PROVING`

- 位置：`cli/tests/cli_e2e.rs:658-672`
- 证据：

```rust
let covered = [(0, "成功"), (1, "校验未通过"), ...];
assert_eq!(covered.len(), 8, "退出码矩阵应被 8 个码完整覆盖");
```

断言对象是本函数内硬编码的局部数组，与 `cli/src/output.rs::CliError::exit_code` 无任何链接。
- 影响：删掉矩阵里任一码、或改坏 `exit_code()` 的映射，本用例仍绿。测试名与文档
  （README、CONTRIBUTING:114）都声称它守护「退出码 0–7 契约」。
- 修复：改为从 `CliError` 变体表驱动生成期望码（遍历变体断言
  `exit_code()` 的**取值集合** == `{0..=7}`），否则删除以免制造虚假信心。

---

#### P1-16｜MSRV `1.82` 是无效承诺，CI 无任何任务在 1.82 上编译　`指纹 CI-MSRV-UNVERIFIED`

- 证据：三个 `Cargo.toml` 都写 `rust-version = "1.82"`；`README.md:32`、
  `CONTRIBUTING.md:26/157` 对外承诺 1.82+（`CONTRIBUTING.md:157` 甚至写「提升需改 CI」）；
  但 `ci.yml` 与 `release.yml` 只有 `dtolnay/rust-toolchain@stable`，
  仓库内**无 `rust-toolchain.toml`**。依赖（clap 4 / serde 1 / tiny_http 0.12 / minijinja 2.24）
  任一 minor 抬高 MSRV，声明会静默失真。
- 修复：matrix 加一维 `rust: [stable, 1.82]`，1.82 上跑
  `cargo check --workspace --all-targets`。

---

#### P1-17｜绝对路径经 API 响应与错误体泄露　`指纹 API-PATH-DISCLOSURE`

- 位置：`cli/src/context.rs:200`；`cli/src/server.rs:731-733`、`:412`、`:498`
- 证据：`format!("文件模板: {}", canonical.display())` 作为 description，
  经 `/api/templates`（`server.rs:195-205`）与 `/api/templates/{name}`（`:238-252`）
  返回给浏览器；`internal_error` 把 `CliError::message` 原样回显（含绝对路径）。
- 触发条件：`curl http://127.0.0.1:8787/api/templates` 即拿到全部模板文件的绝对路径
  （含用户名与项目结构）。
- 修复：HTTP 侧只回模板名，不回磁盘路径；500 统一返回泛化文案，详情写 stderr。

---

#### P1-18｜每请求全树 `stat`，模板量大时 API 延迟线性增长　`指纹 PERF-TREE-STAT`

- 位置：`cli/src/context.rs:308-328`（`tree_stamp`）
- 说明：注册表缓存已按「目录 + 全树 mtime 指纹」实现（较 09-15 的「每请求重建」是实质改进），
  但**缓存命中前仍会 `read_dir` + `metadata` 遍历整棵模板树**。数百个模板时每个 API 请求
  数百次 syscall。
- 修复：只用根目录 + 顶层子目录 mtime 做指纹，或引入显式刷新命令。

---

#### P1-19｜无读超时 + 无并发上限，单连接即可挂死整个服务　`指纹 SERVER-NO-TIMEOUT`

- 位置：`cli/src/server.rs:668-676`
- 说明（已核对 tiny_http 0.12 实现）：`TaskPool::spawn` 在无空闲线程时**新建线程、无上限**；
  请求头由连接线程读取，但**请求体由主循环线程同步 `read_to_end`**。
- 触发条件：`nc 127.0.0.1 8787` 后只发 `Content-Length: 1048576` 的头、不发体 →
  主循环永久阻塞，服务不可用；大量空闲连接 → 线程无界增长。
- 修复：给 body 读加超时（`set_read_timeout` 或等价手段），并限制并发 worker 数。
  （本地工具定位下风险可控，但既然 `--host` 已硬拒非回环，这条是同一防线的自然补全。）

---

#### P1-20｜`extract_template_refs` 嵌套语句体分支零覆盖，组合模板必选参数有漏检风险　`指纹 TEST-EXTRACT-REFS-GAP`

- 位置：`src/extract.rs:158-202`（lcov 显示 `158 159`（for-else）、`166 167`（if 的 else 体）、
  `169-177`（with / set-block）、`179-202`（autoescape / filter-block / block / macro / call-block）
  全部 `DA:...,0`）
- 说明：该函数被 `core/src/registry.rs:121` 用于「递归检查被引用模板的参数，避免组合模板的
  必选参数漏检」——正是撞刀级静默错误的入口。唯一被覆盖的嵌套形态是
  `{% for %}...{% include %}`（`src/lib.rs:601-606`）；现有
  `template_refs_collected_and_deduped`（`src/extract.rs:793`）只测顶层并列引用。
- 修复：补 `{% if %}{% include "a.j2" %}{% else %}{% include "b.j2" %}{% endif %}`、
  `{% macro %}…{% include %}{% endmacro %}`、`{% filter %}…{% include %}{% endfilter %}` 三例。

---

### 2.3 P2 —— 改进项

| 编号 | 位置 | 问题 | 建议 |
| --- | --- | --- | --- |
| P2-1 | `src/filters.rs:44` | `nc_fixed` 走 Rust `{:.N}`（**半偶**舍入），minijinja `round` 走半远离零 → `2.5\|nc_fixed(0)` = `2` 而 `2.5\|round` = `3`；`0.125\|nc_fixed(2)`=0.12、`0.375\|nc_fixed(2)`=0.38 同一位取向相反。**注意：已实测与 Python 源项目 `f"{v:.2f}"` 逐值一致，故不是迁移回归**，是「同引擎内两套舍入语义」的一致性/文档问题 | 在文档写明舍入模式，或改为先 `(v*10^d).round()/10^d`（需补有限性检查防溢出）与 `round` 对齐 |
| P2-2 | `src/filters.rs:44/58` | 负零泄漏：`-0.0 \| nc_fixed(3)` → `-0.000`、`-0.0 \| nc_strip` → `-0`（实测） | `nc_signed` 已有归一（`:86`），`nc_fixed`/`nc_strip` 同样加上 |
| P2-3 | `src/filters.rs:86` | `nc_signed` 归一不完整：`-0.0001 \| nc_signed(3)` → `-0.000`（实测），与 `:61-62` 文档承诺相悖；归一应放在舍入**之后** | 舍入后判 `== 0.0` 再置正零 |
| P2-4 | `src/filters.rs:122` | `value.trunc() > i64::MAX as f64` 差一：`i64::MAX as f64` 恰为 2^63，该值通过检查后被 `as i64` 饱和成 `i64::MAX`（实测），与注释「超界直接报错」不符 | `>` 改 `>=` |
| P2-5 | `src/filters.rs:93` | 文档与实现直接矛盾：注释写「输入为浮点数时截断小数部分取整」，`:131` 明确拒绝小数 | 改注释（`#![warn(missing_docs)]` 下属失效文档） |
| P2-6 | `core/src/model.rs:174/227` | `n as i64` 对 `{type: integer, value: 1e20}` 静默饱和为 `i64::MAX`（`is_finite() && fract()==0.0` 均通过） | 加 `n <= i64::MAX as f64` 边界判断后报错 |
| P2-7 | `src/extract.rs:23` | `BUILTIN_GLOBALS` 中的 `lipsum`/`cycler`/`joiner` **不是** minijinja 提供的全局（实际只有 `range`/`dict`/`debug`/`namespace`）。同名参数会被静默排除出 `undeclared` | 删除或改为可配置 |
| P2-8 | `src/extract.rs:66` | 注释称「minijinja 内部自带 JIT 编译缓存」，但 `template_from_named_str` 每次新建 `CompiledTemplate`、无缓存；`renderer.rs:160` 的 `render` 每次完整重编译 | 改注释；若该路径在热循环中，改用 `get_template` |
| P2-9 | `src/extract.rs:114/126` | 两个入口各自完整 walk 并同时构建 `all` 与 `undeclared`，再丢弃一半；core（`registry.rs:120`）与 cli（`templates.rs:137/146`）同时需要两者时即两遍遍历 | 加 `extract_both` |
| P2-10 | `src/extract.rs:367`、`:246`、`src/renderer.rs:185` | 死代码/冗余：`c.declare("loop")`（`record` 在 `RESERVED_NAMES` 处已 return）；`Collector::new(_src)` 参数未用；`add_template_owned(name.clone(), source.clone())` 多克隆一次源码 | 清理 |
| P2-11 | `src/error.rs:194` | `extract_undefined_var_name` 只拒绝紧跟 `.`/`[`，而 Emit 指令 span 覆盖整个 `{{ … }}` → `{{ x \| default(y) }}` 在 y 缺失时报「未定义变量 'x'」（`[待验证]`，需构造用例） | 要求提取的标识符覆盖 trim 后**整个** range 才采纳 |
| P2-12 | `src/error.rs:370` | 嵌套 include/extends 的子模板错误被 minijinja 包成 `BadInclude`，`err.kind()` 不再是细分变体 → 组合模板上 `TplError` 的细分变体全部失效，只剩 `Render`（`[待验证]`） | 沿 `source()` 链取最内层错误的 `kind()` |
| P2-13 | `src/error.rs:238` | `MAX_ERROR_CHAIN = 8`：嵌套超 8 层时根因被截断且无提示 | 截断处补 `…` |
| P2-14 | `src/lib.rs:1554/1617` | 两个 fuzz 测试只断言「不 panic」、不断言返回值，易造成「提取器已被 fuzz 验证」的错觉 | 补返回不变量断言 |
| P2-15 | `core/src/derive.rs:164`、`core/src/registry.rs:175/654` | `derived_names` / `invalidate_analysis` 在非 target 目录零引用；`install_builtins` 用 `.expect()`（库代码中的 panic 路径）；`TemplateEntry::source_text` 为 `pub`，改写后靠调用方自觉调 `invalidate_analysis` | 收敛可见性；`.expect()` 换 `Result` |
| P2-16 | `core/src/validate.rs:290` | `validate_with_vars` 传 `template_name=None` → registry 路径下所有报错只有「第 L 行第 C 列」，组合模板无法定位片段 | 增加名字参数 |
| P2-17 | `core/src/validate.rs:666` | 对 NaN 会额外报一条 `NotInteger`（`NaN.fract() != 0.0` 为真），噪声 | NaN 时跳过整数性检查 |
| P2-18 | `core/src/manifest.rs:40` | `PARAMS_SCAN_LINES = 200` 上限外的 `{# PARAMS: #}` 被**静默忽略** → 该模板全部类型约束丢失 | 超限时产出 warning |
| P2-19 | `core/src/manifest.rs:438` vs `:358` | 注释称「复用 `ManifestFile` 的严格解析（`deny_unknown_fields`）」，但 `ManifestFile` **没有**该属性 → 顶层拼错的键被静默忽略 | 加 `deny_unknown_fields` 或改注释 |
| P2-20 | `core/src/model.rs:236` | `as_f64()` 在 `>2^53` 整数上丢精度，`min`/`max` 比较可能失真（CNC 量级远不到）`[待验证]` | 文档标注上限，或大整数走精确比较 |
| P2-21 | `cli/src/server.rs:3-5` | 模块注释仍称「非回环由命令层打印警告（本模块不做判断）」，而 `listen_addr`（`:617-628`）已直接拒绝——注释与实现矛盾，后续维护者可能「恢复」成只告警 | 同步注释 |
| P2-22 | `cli/src/server.rs:367-390` vs `cli/src/commands/validate.rs:63-87` | 报告映射两份实现，字段名与 level 映射各写一遍（`inspect` 已正确复用 `server::spec_json`） | 收敛到 core 的一个 `Serialize` DTO |
| P2-23 | `cli/src/context.rs:271-286` | `find_template_file` 的「仅 CLI」契约只靠注释，且是 `pub`、仍支持绝对路径与 `..`。当前无调用点可达（已验证），属「靠约定」而非「靠类型」 | 改 `pub(crate)` 或拆分能力 |
| P2-24 | `cli/src/commands/ui.rs:16-20` | `--open` 仍先于真正 bind；`--port 0` 时 `browser_url` 显示 `:0` | 先 bind 成功再开浏览器，端口 0 时回读 `server_addr()` |
| P2-25 | `ui/index.html:1933` | Bool 参数恒提交 `false`（未触碰的复选框也发 `false`），而 CLI 会省略该键 → 宽松/条件必选判定可能与 CLI 不一致 | 仅在用户交互过或初始值非 undefined 时提交 |
| P2-26 | `ui/index.html:1951-1957` | 有 `renderSeq` 序列保护（正确），但无 `AbortController`，后端阻塞时旧请求堆积 | 加取消 |
| P2-27 | `.github/workflows/ci.yml:73/76` | `cargo install cargo-audit --locked` 未固定版本（换新版即整条 CI 红），且三平台各编译一遍；`cargo audit` 未加 `--deny warnings`；无 `deny.toml` | 改用 `taiki-e/install-action`，补 `deny.toml` |
| P2-28 | `ci.yml:65` | `cargo test --workspace --all-targets` **不跑 doctest**（实测：去掉 `--all-targets` 才有 `Doc-tests` 段），而 `README.md:513` 声称「单元 + 集成 + 文档」全跑 | 补 `cargo test --workspace --doc` |
| P2-29 | `ci.yml:65/76` | `Cargo.lock` 已提交但所有步骤都不带 `--locked` | 至少 `test` 与 `audit` 加 `--locked` |
| P2-30 | `core/tests/integration.rs:36-42` | `NCTOOL_UPDATE_GOLDEN` 命中即写文件并 `return`，跳过全部断言。当前 workflow 未设置，但一旦被写进 `env:` 或 `.cargo/config.toml`，21 组 golden 会静默变成「刷新器」 | 加 `assert!(std::env::var_os("CI").is_none(), ...)` 前置守卫 |
| P2-31 | `tests/golden/*.report.txt` | 21 份报告文件 md5 全同，内容恒为「校验通过：无问题」——21 组基线在报告维度只有 1 份信息量 | 至少 2 组故意缺参/类型不符的负向 golden |
| P2-32 | `cli/tests/cli.rs:254-272` | 注释称「CLI 渲染输出与 core 管线逐字节一致」，但 `:272` 是硬编码字符串、不读 `tests/golden/*.nc`。当前恰好相同，模板一改需手工同步两处且无测试能发现 | CLI 侧改读同一份 golden |
| P2-33 | `tests/parsing.rs:358-367` | `all_math_filters_render` 用 `assert!(out.contains("2"))` 等弱断言（9 个过滤器输出拼一行，`"2"` 可由任一满足） | 逐过滤器 `assert_eq!` 精确值 |
| P2-34 | `output/`、`NVIDIA Corporation/`、`lcov.info`、`gcm-diagnose.log` | `output/` 16 个文件（含 3 张 PNG、4 个 `.py`、`模型交接文档.html`，共 401K）已被 git 跟踪且未进 `.gitignore`；根 `Cargo.toml:19 exclude` 未收窄（`PROJECT_STATUS.md:217/285` 已登记「tpl 包 925 KiB 含仓库级杂物」）；`NVIDIA Corporation/umdlogs` 是残留空目录 | 移出仓库或补 `.gitignore` / `exclude` |
| P2-35 | `docs/CONTRIBUTING.md:108/82`、`README.md:545` | 测试数量四处漂移（344 / 516 / 492；实测 `#[test]` ≈ 522） | 改为只写「见 CI job summary」 |
| P2-36 | `scripts/check_docs_links.py` | 已文档化用法但无任何 workflow 调用点（`PROJECT_STATUS.md:215` 已登记「工具闲置」） | 接入 CI 或删除 |
| P2-37 | `cli/tests/cli_e2e.rs:97` | 注释「创建本仓库根目录下唯一的临时目录」，实现用 `std::env::temp_dir()` | 改注释 |

---

## 3. 改进方案（按批次）

### 批次一：堵住静默出错 ✅ **已完成（2026-09-18）**

| 项 | 修复 | 回归测试（均已反向验证：回滚实现 → FAILED） |
| --- | --- | --- |
| P0-2 派生链式依赖 | `derive.rs` 改为按依赖顺序（重复扫描到不动点）计算，源参数若也是派生参数则取已算出的派生值；成环返回新增的 `DeriveError::Circular` | `chained_derive_uses_computed_source_not_stale_user_value`、`chained_derive_uses_derived_value_not_spec_default`、`circular_derive_errors_instead_of_silently_using_stale_value` |
| P0-3 `required_if` 短路 | `validate.rs` 中声明了 `required_if` 的参数，其必选性不再受 `var.optional`（模板内联 `default`）影响；规格显式 `default` 仍算兜底 | `required_if_is_not_short_circuited_by_template_inline_default` + `required_if_untriggered_branch_still_skips_with_inline_default`（守边界） |
| P0-4 列表有限性 | `check_finite` 递归列表元素，消息带下标路径 `[1][0]`，沿用 32 层深度上限 | `nan_inside_list_is_rejected` |
| P1-5 规格默认值有限性 | `check_spec_defaults` 补 `check_finite_value` | `nan_spec_default_is_rejected` |
| P1-6 空前缀 | 新增 `non_empty_config`（键缺失与空串同等回退）；`validate_config_keys` 对 String 键补非空校验 | `empty_line_number_prefix_falls_back_to_default`、`empty_program_prefix_falls_back_to_default`、`empty_string_value_warns` |

门禁实测（2026-09-18）：

- `cargo test --workspace --all-targets`：**536 项全部通过**（新增 10 项）
- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`：均通过
- 改动集中在 `core/`（4 文件，+417/-54），`DeriveError` 新增 `Circular` 变体
  （该枚举本就 `#[non_exhaustive]`，非破坏性变更）

> **反向验证方法**（建议固化为约定）：把实现临时回滚到 `git show HEAD:<file>`，
> 保留新测试跑一遍，确认真红后再恢复。本批次 10 项新测试全部经此验证。

### 批次二：修好度量闭环（1 天）

1. 按 P0-1 选定口径并让阈值反映生产代码；同步 `CONTRIBUTING.md` / `README.md` /
   `RELEASE.md` 的阻断性表述（P1-14）。
2. `exit_code_matrix_is_fully_covered` 改为表驱动真断言（P1-15）。
3. CI 加 MSRV 维度（P1-16）、`--locked`、`--deny warnings`（P2-27/29）、
   `cargo test --doc`（P2-28）。
4. `NCTOOL_UPDATE_GOLDEN` 加 CI 守卫（P2-30）。

### 批次三：Web UI 加固 ✅ **已完成（2026-09-18）**

| 项 | 修复 | 回归测试 |
| --- | --- | --- |
| P1-1 Web UI XSS | `esc()` 补 `"` `'`；`tplCardHtml` / `fieldHtml` / `optionsHtml` / `renderMachineSel` / crumb 逐点补转义；新增 `selEsc` 让 `querySelector` 侧的属性值与 `esc` 写入侧配对 | 浏览器实测（见下） |
| P1-2 目录链接环 | `collect_templates` 目录分支补 `canonicalize` + `starts_with(root)` 校验与 `BTreeSet<PathBuf>` visited 集合 | `symlink_cycle_is_broken_and_templates_still_collected`、`symlink_escaping_root_is_skipped`（`#[cfg(unix)]`） |
| P1-3 选项类型 | 输入处理器存 `Number`；`API.render` 出口统一过 `normalizeOpts` | 浏览器实测（步进改 25 → `N0025/N0050/N0075`） |
| P1-4 清单默认值分歧 | 手写 `impl Default for TemplateMeta`（`visible: true` / `output_extension: ".NC"`），与 serde 路径对齐 | `template_absent_from_manifest_is_visible_with_default_extension`（同时断言两条路径一致） |
| P1-8 分类缺口 | `parse_category` 补 `grooving`/`切槽`；空 query 视为不筛选；前端 `CATS` 同步（两份 `index.html`） | `parse_category_covers_every_core_variant`（遍历 core 全部分类）、`empty_category_query_means_no_filter` |
| P1-9 format 类型 | 区分「键缺失」与「类型错误」，后者 400 | `render_options_format_must_be_string` |
| P1-17 路径泄露 | 兜底描述改用相对键而非绝对路径；500 只回泛化文案、详情写 stderr | `internal_and_cli_errors_map_status`（断言 500 正文不含路径） |
| P2-21 过期注释 | 模块头「非回环由命令层警告」与实际（`listen_addr` 直接拒绝）矛盾，已同步 | — |

**未做（附理由，不是遗漏）**

- **P1-1 的 CSP 收紧**：去掉 `'unsafe-inline'` 要把内联 `<script>`/`<style>` 外置为
  同源文件（或每响应注入 nonce），属独立改造。本轮先消除**注入点本身**，
  并在 CSP 注释里写明「转义是这条取舍成立的前提、改插值必须一并复核」。
- **P1-19 body 读超时**：tiny_http 0.12 不暴露底层 socket，`set_read_timeout` 无从下手；
  改用独立线程读也要把 `Request` 移进线程才能满足 `'static`，而响应又必须由持有
  `Request` 的一方发出 —— 得重做 `serve` 的请求循环。风险面本已受限（非回环直接拒绝，
  只有本机进程可达），宜作独立一项，不塞进安全修复提交。
- **P1-18 每请求全树 `stat`**：与 P1-19 同属服务层，一并留待。

**XSS 反向验证（浏览器实测，2026-09-18）**

临时模板目录放一个 `{# PARAMS: #}` 参数名为 `x"onmouseover="window.__XSS_ATTR=1`
的模板（Windows 文件名不允许 `<`，故走参数名这条注入路径），启 `nctool ui` 后：

| 检查 | 结果 |
| --- | --- |
| 页面内出现 `[onmouseover]` / `<img>` 注入元素 | **无** |
| `data-param` 属性回读 == 原始参数名 | ✅（`&quot;` 解析后与原名一致） |
| `querySelector` 能回找到该输入框、值被 `collectParams` 收走 | ✅（转义/回读两侧配对） |
| 用**旧** `esc()` 拼同样 HTML | 确实注入 `onmouseover="window.__XSS_ATTR=1"` —— **证明漏洞真实存在** |
| 渲染仍正常 | ✅（`G0 X0 Z0`，状态「就绪」） |


### 批次四：一致性清理 ⚠️ **P1 全清，P2 部分完成（2026-09-18）**

**P1（全部完成）**

| 项 | 修复 | 回归测试 |
| --- | --- | --- |
| P1-7 同名模板静默替换 | `resolve_registry` 命中同名且**不是同一文件**时，改用路径作注册名把用户给的文件注册进去（原先是直接复用注册表条目 → 渲染出另一份程序） | `explicit_path_that_collides_with_registered_name_wins` |
| P1-10 断管道 panic | 该分支的 `println!` 改走 `write_stdout_quiet`（提为 `pub(crate)`）；实测退出码 101 → 1 | 真实进程 `--format json \| :` |
| P1-11 include 闭包规格优先级 | 先并入本层 `params` 再递归（原顺序让离主模板最远的声明胜出，与文档相反） | `include_closure_spec_precedence_is_nearest_to_main` |
| P1-12 孤儿清单键 | 新增 `TemplateManifest::orphan_keys`，加载时对未命中键打 warning | `orphan_keys_reports_unmatched_entries` |
| P1-13 稀疏覆盖无法清空 | 约束字段改 `Option<Option<T>>` + `double_option`，`null` = 清空 | `manifest_null_clears_inherited_field` |
| P1-20 引用遍历分支零覆盖 | 补 11 个分支的表驱动用例；实现在补测前即为正确，本项是纯覆盖缺口 | `template_refs_traverse_every_nested_body` |

**P2（本轮完成 12 条，余者仍开放）**

已完成：P2-2 负零泄漏、P2-3 `nc_signed` 归一位置、P2-4 `nc_pad` 上界差一、
P2-5 `nc_pad` 文档与实现矛盾、P2-1 舍入模式文档化、P2-6 `as i64` 静默饱和、
P2-7 `BUILTIN_GLOBALS` 含非 minijinja 全局、P2-17 NaN 额外报 `NotInteger`、
P2-18 `PARAMS` 块超限静默截断、P2-19 `ManifestFile` 缺 `deny_unknown_fields`、
P2-21 模块注释与实现矛盾、P2-27 的「补 `deny.toml`」（**见下**）、
P2-36 `check_docs_links.py` 接入 CI。

> **P2-27 的「补 deny.toml」是误记，未照做**：`deny.toml` 是 `cargo-deny` 的配置，
> 而 CI 用的是 `cargo audit`（读 `.cargo/audit.toml`）。建一个没人读的文件正是
> 本报告 P2-36 批评的「工具闲置」。`cargo audit --deny warnings` 已实测有效，
> 该条视为不成立；若确要 license/source 策略，应单独评估引入 `cargo-deny`。

仍开放（按文件归并，未做）：

- **P2-8 / P2-10 / P2-9 同族**（`src/extract.rs`）：注释已改（不再断言 minijinja 的
  缓存行为），但 `extract_both` 与死代码清理未做 —— 属性能/整洁，无正确性影响。
- **P2-11 / P2-12**（`src/error.rs`）：原报告标 `[待验证]`，需先构造用例确认是否
  真存在，不宜照单改。
- **P2-16 / P2-24**（`validate_with_vars` 的名字参数、`ui --open` 与 `--port 0`）：
  可用性问题，改动面比看起来大。
- **P2-14 / P2-31 / P2-32 / P2-33**（测试质量：fuzz 返回值断言、负向 golden、
  CLI 改读同一份 golden、`all_math_filters_render` 弱断言）：价值明确，单独一批做。
- **P2-25 / P2-26**（UI 的 Bool 恒提交 `false`、缺 `AbortController`）：与 Web UI
  同批。
- **P2-34**（`output/` 等仓库杂物、发布包收窄）：与发版流程一起做（`PROJECT_STATUS`
  §8 第二/三优先）。
- **P2-35 的剩余位点**：`CONTRIBUTING` §4 与 README 已改为不硬编码项数，
  其余散落处待清。

**不并入批次四，单独排期**

- **P1-18**（每请求全树 `stat`）与 **P1-19**（无读超时 / 无并发上限）：同属 `serve`
  请求循环的服务层改造，放一起做才划算，且都不宜夹在安全修复里。
- **P1-1 的 CSP 收紧**（去掉 `'unsafe-inline'`）：需把内联 `<script>`/`<style>`
  外置，与上面两项同属服务层。

**关于覆盖率门**：生产口径已从 88.65% 升至 **89.49%**（P1-20 补齐了一批零覆盖分支）。
**未上调阈值门**（仍 88%）：89.49% 是本机 Windows 实测，CI 在 Ubuntu 上跑，
余量 1.49pt 未必能跨平台兑现 —— 建议先看一次 Ubuntu 的实测数字再定。


---

## 4. 已核实无问题的方面（不要在这些地方「修」出新问题）

| 项 | 结论 |
| --- | --- |
| 09-05 三个 P0 | **全部已修复**：LFI（HTTP 只走注册表 `get(&name)`，`find_template_file` 无 HTTP 调用点）、demo 模式（`mode` 按协议判定 + 启动即拉真实 API）、DTO 字段（统一 `level/message`，全文件已无 `level === "warn"`） |
| 非回环监听 | **比建议更严**：`listen_addr` 直接拒绝，而非只告警；`--host ::1` 已用 `SocketAddr` 正确格式化 |
| 跨站防护 | `Origin` + `Sec-Fetch-Site` 校验齐备；OPTIONS 落 404 且无 ACAO；安全头（CSP/nosniff/Referrer-Policy）对**所有**响应生效，含 403/404/500 |
| 请求体上限 | `take(MAX+1)` + 413，有测试 |
| `--param` 归一 | 后端 `args.rs:20-59/97-117` 与前端 `:1850-1863` **同序**；`scripts/param_parity_cases.json` 被 Rust 测试（`args.rs:463-487`）与 `check_param_parity.mjs` 双向消费，脚本覆盖两份 HTML。**全仓最好的防漂移设计，不要动** |
| 两份 `index.html` | `diff` 为空，`cli/tests/cli.rs:890-898` + parity 脚本双重拦截 |
| `path_loader` 逃逸 | `safe_join`（minijinja `loader.rs:180`）拒绝以 `.` 开头的段与含 `\` 的段，自定义闭包再挡空名/`:`/绝对路径。**不要动** |
| 提取器作用域模型 | `walk_stmt`/`walk_expr` 对 minijinja 2.24 的枚举**穷尽覆盖**（无 `_` 分支，编译器保证）；with 块「求值右值→绑定目标」逐条交错、`SetBlock` 先体后绑定、for-else 在循环帧外——与 VM 逐条核对一致。**不要动** |
| `default`/`d` 兜底判定 | 逐分支复核（含嵌套 `default` 链、括号、属性/下标链、过滤器参数位置），**未发现「误判为可选」（危险方向）** 的情况 |
| `line_col_at` | 对 CRLF、tab、多字节 UTF-8 与 lexer 逐字符计数口径一致，`off` 有 `min` + `is_char_boundary` 保护，不会 panic |
| 白名单与约束分离 | `check_value_options` 独立于 `check_value_constraints`，调用顺序「类型 → 白名单 → 区间/整数」正确，字符串枚举不会被 `as_f64()` 提前返回吞掉 |
| `IssueKind` 语义 | `ConditionalSkipped`(Info) / `Missing`(Error) / `SpecInert`(Warning) / `Unused`(Warning) 在类别、级别、消息三处均正确区分 |
| 边界与数值 | 区间含端点；`1.0000001` 因 `fract()!=0` 被拒、未误判为整数；`"5"` 与 `5` 不做隐式互转（`matches_option` 有意禁止） |
| 行号安全 | `checked_add` 防溢出、`line_digits` clamp 到 `[1,32]`、截断后仍保留行内容 |
| 退出码 | `output.rs:45-57` 矩阵完整，`216-235` 逐项钉住（但见 P1-15 的自证测试） |
| stdout/stderr 分流 | `render.rs:41-49` 报告走 stderr、G-code 走 stdout，管道可用（仅 P1-10 一处例外） |
| `inspect --format json` | 载荷在 `data` 键下，符合约定 |
| golden 配对与行尾 | 21 `.nc` + 21 `.report.txt` 双向无孤儿；`golden_files_are_lf_only` 断言 `checked >= 42` 且逐字节拒绝 `\r`；失败时打印左右全文 diff |
| 属性测试 | `tests/extract_invariant.rs` 用零依赖 LCG，300 用例，单侧不变量已在文件头说明，含生成器退化保护 |
| 生产 panic 面 | 0 处 `unsafe`、0 处 `panic!`；`unwrap/expect` 仅 `server.rs` 3 处静态常量与 `commands/mod.rs:39` 的不可达分支 |

---

## 5. 与历史评审的关系

| 09-05 报告项 | 本轮状态 |
| --- | --- |
| P0-1 WEBUI-LFI-001 | ✅ 已修复（且 HTTP 侧不碰文件系统，绝对路径/UNC/ADS/8.3 短名一并失效） |
| P0-2 WEBUI-CONTRACT-001 | ✅ 已修复（`file://` 双击保留 demo 是刻意取舍，有说明） |
| P0-3 VALIDATION-DTO-001 | ✅ 已修复 |
| P1-1 REMOTE-EXPOSURE-001 | ✅ 已修复（改为硬拒，优于建议） |
| P1-2 UI-E2E-HEALTH-ONLY | ✅ 已修复（`cli/tests/cli.rs:696-800` 覆盖真实 API） |
| P1-3 OPTIONS-SILENT-FALLBACK-001 | ⚠️ **部分修复** → 残留即本轮 P1-9 |
| P2 每请求重建注册表 | ⚠️ 已缓解（加 mtime 指纹缓存）→ 残留 P1-18 |
| P2 无读超时/并发保护 | ❌ 仍存在 → 本轮 P1-19 |
| P2 `--host ::1` 拼接 | ✅ 已修复 |
| P2 先 `--open` 后绑定 | ❌ 仍存在 → 本轮 P2-24 |

本轮**新增**的高价值项集中在 09-15 架构评估未覆盖的**语义层**：派生链、`required_if` 交互、
列表递归有限性、机床字符串配置、覆盖率口径。
