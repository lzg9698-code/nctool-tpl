# 模板编写指南

> 适用版本：nctool-tpl ≥ 0.3.2
> 模板基于 [Jinja2](https://jinja.palletsprojects.com/)（用 [minijinja](https://github.com/mitsuhiko/minijinja) 引擎）。
> 本指南聚焦**面向 NC/G-code 的实战写法**与本项目独有的约定，**通用 Jinja 语法**请参考上游文档。

## 1. 第一个模板

`templates/turning/demo_gcode.j2` 是一个完整示例，可直接 `cargo run --example demo` 跑通。

```jinja
O{{ prog | nc_pad(4) }} ({{ part_name | default('') }})
G90 G54 G17
M3 S{{ spindle_speed | nc_strip }}
{% for hole in holes %}
G0 X{{ hole.x | nc_fixed(3) }} Y{{ hole.y | nc_fixed(3) }}
G1 Z{{ hole.depth | nc_fixed(3) }} F{{ feed | default(0.15) | nc_fixed(3) }}
G0 Z5.0
{% endfor %}
M5
M30
```

最小要求：

```jinja
{{ x }}                         {# 引用变量 #}
{{ x | nc_fixed(3) }}           {# 应用过滤器 #}
{% for item in items %}...{% endfor %}    {# 控制流 #}
{# 这是模板注释 #}
```

变量名是**裸标识符**，下划线开头（如 `_internal`）**不**会自动私有化；想"内部用"的变量用 `{% set %}` 声明。

## 2. 必选 vs 可选：模板里看不出来，看 `inspect`

模板本身**不声明**参数是否必选。nctool 用启发式判定（见 `core/src/registry.rs` 与 README「可选 / 必选判定规则」）：

- **可选**：`x | default(v)`、`x | d(v)`、`x is defined`、`x is undefined` 的**直接裸变量操作数**
- **必选**：其他引用（如 `{{ x }}`、`{{ x / 2 }}`、`(a+b) | default(1)`、`a.b is defined`）

判定粒度是**变量级**：同一变量既有可选引用又有必选引用 → 标为**必选**。

判定策略是**宁多勿漏**（把可疑的标为必选）—— 这样上层校验会放行、严格模式渲染却失败、产出**不完整 G-code** 的情况被堵住。

查看模板的参数表：

```bash
nctool inspect drill_cycle
# 输出:
#   必选参数（5）:
#     x  行 1 列 24
#     ...
#   可选参数（0）:
```

## 3. NC 数值格式化过滤器（**最常用**）

G-code 对数值格式敏感，库内置四个专用过滤器。

| 过滤器 | 用法 | 输入 | 输出 | 用途 |
| --- | --- | --- | --- | --- |
| `nc_fixed(N)` | `{{ x \| nc_fixed(3) }}` | `21.0` | `21.000` | 固定小数位的坐标值 |
| `nc_signed(N)` | `{{ x \| nc_signed(3) }}` | `21.0` | `+21.000` | **增量坐标/旋转量**（需显式正号） |
| `nc_strip` | `{{ x \| nc_strip }}` | `21.0` | `21` | 去尾零，避免 `X21.0`（部分控制器不接） |
| `nc_pad(N)` | `{{ n \| nc_pad(4) }}` | `1` | `0001` | 程序号/行号前导零 |

所有 NC 过滤器对 **NaN/Inf 报错**（防止非法数值写入 G-code）。该防线仅覆盖本库注册的过滤器；裸 `{{ x }}` 输出或 minijinja 内建操作（如 `round`、算术）产生的 NaN/Inf 不经此校验——先用 `nctool validate` 拦。

边界与坑：

- `nc_pad(宽度)` 上限 **1024**；`nc_fixed`/`nc_signed` 小数位上限 **32**。超出即报错（避免巨量分配导致 abort）
- `nc_pad` 输入必须是整数或整值的 `Number`（`5.0` 行，`5.5` 不行）—— 否则报错

### 3.1 `nc_signed` 只用于有符号语义的值

`nc_signed` 与 `nc_fixed` 的唯一差别是**正数与零也输出 `+`**：

```jinja
{{ 21.0  | nc_signed(3) }}   {# → +21.000 #}
{{ -4.5  | nc_signed(3) }}   {# → -4.500 #}
{{ 0.0   | nc_signed(3) }}   {# → +0.000 #}
```

**只在以下场景用它**：

- **增量坐标**（`G91` 段）——部分控制器要求显式正号，省略号会被误判为绝对值
- **旋转量**（如 `AROT` 的角度）——同上
- 工艺文档里习惯带符号的注释

**不要**用它格式化直径、长度、进给、转速这类本无符号语义的值
——`X+50.000` 虽然多数控制器能接受，但会让程序可读性变差，
且与图纸标注习惯不符。

> 冷知识：`nc_signed(-0.0)` 输出 `+0.000` 而非 `-0.000`。
> 控制器对负零的处理不一致，且 `-0.000` 在图纸上没有意义，因此做了归一。

## 4. 数学过滤器

全部基于 `f64` 标准库，零额外依赖：

**弧度制**（与 Rust 标准库一致）：
`sin` `cos` `tan` `asin` `acos` `atan` `sqrt` `exp` `ln` `log10` `pow` `floor` `ceil`

**角度制**（工艺图纸用，**推荐**）：
`sin_d` `cos_d` `tan_d` `asin_d` `acos_d` `atan_d`　（`_d` = degrees）

**有限性校验**：所有数学过滤器对结果做 `is_finite()` 检查；`sqrt(-1)`、`ln(0)` 立即报错，避免非法坐标静默写入 G-code。

### ⚠️ 角度制 vs 弧度制（**最容易撞刀的地方**）

图纸上的角度是**度**，而裸 `sin`/`cos`/`tan` 按**弧度**算。两者结果差异巨大：

| 写法 | 结果 | |
| --- | --- | --- |
| `{{ 30 \| sin_d }}` | `0.5` | ✅ 正确 |
| `{{ 30 \| sin }}` | `-0.988` | ❌ 把 30° 当 30 弧度 |
| `{{ (30 * pi / 180) \| sin }}` | `0.5` | ✅ 手工换算，等价但啰嗦 |

**最危险的是静默出错**：`sin(30)` 不报错，只是算出一个错误数字，
而这个数字会直接变成 G-code 里的坐标——**撞刀**。

因此：

- **新模板一律用 `_d` 后缀**：`{{ angle | sin_d }}`
- 裸 `sin`/`cos`/`tan` 只在确认输入本就是弧度时使用
- 从 Python/Jinja2 迁移模板时**特别小心**：源项目常把
  `math.sin(math.radians(x))` 暴露为 `sin`，也就是**那个 `sin` 是度制**，
  而本库的 `sin` 是弧度制——**同名不同义**，必须逐处改成 `sin_d`

```jinja
{# ✅ 推荐：意图明确 #}
G1 X{{ (r * (angle | sin_d)) | nc_fixed(4) }}

{# ✅ 也可以：显式换算 #}
G1 X{{ (r * ((angle * pi / 180) | sin)) | nc_fixed(4) }}

{# ❌ 危险：把度直接喂给弧度制过滤器 #}
G1 X{{ (r * (angle | sin)) | nc_fixed(4) }}
```

> 反三角同样成对：`asin_d(0.5)` → `30`（度），`asin(0.5)` → `0.5236`（弧度）。
> 需要"从比值求角度再写进 G-code"时用 `_d` 版本。

### 4.1 字典查表：用下标不用 `.get()`（约定 R6）

minijinja 的 map **只有下标访问，没有任何方法**——`.get()` / `.keys()` /
`.items()` / `.values()` 全部不可用（Python Jinja2 里正常，在这里会报
`unknown method: map has no method named get`）。

`dict.get(k, d)` 的等价写法是 `m[k] | default(d)`：

```jinja
{% set tip_depth_map = {'B4': 8.51, 'DM24': 29.61} %}

{# ❌ Python Jinja2 写法，minijinja 报错 #}
{% set d = tip_depth_map.get(tip_model, 29.61) %}

{# ✅ 等价写法：下标缺失返回 undefined，再由 default 兜底 #}
{% set d = tip_depth_map[tip_model | default('DM24')] | default(29.61) %}
```

> 注意第二层 `| default(...)` **不能省**：Strict 模式下裸 `m[缺失键]` 会直接报错。
> 需要遍历键时没有 `keys()`，可改用列表字面量（`{% for k in ['B4','DM24'] %}`）
> 或把数据整理成 `[{name: ..., value: ...}]` 的列表形式。

## 5. 引用机床配置

通过 `{{ machine.<key> }}` 引用：

```jinja
{{ machine.coolant_on }}   {# → M8（默认）/ M7（开雾冷等）#}
{{ machine.spindle_off }}  {# → M5 #}
{{ machine.line_number_digits | int }}  {# 整数键需要显式 int #}
```

完整配置键清单与各键用途见 [MACHINE_CONFIG_GUIDE.md](MACHINE_CONFIG_GUIDE.md)。

## 6. 多模板：`include` / `extends` / `from import`

```jinja
{# 模板 A: header.j2 #}
O{{ prog | nc_pad(4) }}
G90 G54

{# 模板 B: main.j2 #}
{% include "header.j2" %}
G1 X{{ x }} F{{ feed }}
M30
```

`include` 解析时会**穿透到子模板校验**——子模板的必选参数也必须由调用方提供，且**环引用会被防护**（A include B、B include A → 报错）。

子模板通过 `add_template`（内存）或 `set_path_loader`（文件目录）注册。`include "header.j2"` 按名查找：先查内存注册表，再查 path loader 目录。

## 7. 校验 / 渲染两层校验

- **`nctool validate <template>`**：渲染前校验必选参数齐全、类型匹配、数值在 `[min, max]` 区间内、值有限。输出结构化报告。
- **`nctool render <template>`**：先校验再渲染，校验失败阻断生成。
- **`--lenient`**：除 NaN/Inf 外，校验错误降级为提示，仍生成 G-code（用于"宽容场景"，如柔性模板）。

```bash
nctool validate drill_cycle --param x=1 --param y=2 \
    --param r_plane=3 --param depth=-5 --param feed=100
# 校验通过：无问题

nctool validate drill_cycle   # 缺必选
# 错误 [x] 必选参数缺失（...）
# 退出码 1
```

## 8. 调试技巧

- **变量看值**：`{{ x | debug }}` 不存在，但可以 `{{ "X=" ~ x }}` 在 G-code 里夹带可见标记
- **看参数表**：`nctool inspect <template>` 列出必选/可选参数 + 行列定位
- **看校验报告**：`nctool validate ...` 输出每条错误的「参数名 + 行 + 列」
- **看渲染过程**：把模板先 `nctool render --format json` 拿到 `data.report.issues`，定位是校验/渲染哪一层的问题
- **模板存为文件**：`nctool templates new <name>` 在 `templates/` 下生成骨架

## 9. 反模式

| 反模式 | 后果 | 正解 |
| --- | --- | --- |
| `{{ x / 2 \| default(0) }}` | 算 `x/2` 时 `x` 必选；`default` 兜不住 | `{{ (x \| default(0)) / 2 }}` |
| `{{ a.b \| default(1) }}` | `a` 必选（`a.b` 先求值） | 拆成两步：`{% if a is defined %}{{ a.b }}{% endif %}` |
| `{{ x \| default(1) }}` 给 `Number` 用，又想传字符串 | 类型不匹配报错 | 模板里 `{{ x \| default(1) | int }}` 强转 |
| 模板里手写 `N0010 N0020 ...` | 双重编号 | 用 `--line-numbers` 让后处理统一生成 |
| 在 `program_header` 之后又写一个 `M30` | 程序有两个结束 | 把 M30 放在 `program_footer` 模板里，**链式**：`program_header` → 工序 → `program_footer` |
| `{{ prog }}` 期望 `O0001` 但 `prog=1.7` | 输出 `O1.7`（非法） | 用 `nc_pad` + `ParamKind::Integer` 限定 |

## 10. 模板调试清单

发布或投入前，建议过一遍：

- [ ] `nctool inspect <template>` 列出的必选参数是否都符合工艺预期
- [ ] `nctool validate` 缺所有必选参数时，能定位到正确的「行+列」
- [ ] 边界值校验：进给率 = 0、深度 > 0、刀具号 = 0 等
- [ ] 中文字符串注释不依赖 ASCII 清洗（默认关闭即可正常显示）
- [ ] 多行输出用 `strip_blank_lines` 清理空行
- [ ] 程序号/行号交给后处理（`--line-numbers`），不手写

## 11. 相关文档

- [README.md](../README.md) — 顶层 API 概览 + 判定规则详细表
- [docs/ARCHITECTURE.md](ARCHITECTURE.md) — 解析 / 提取 / 渲染 / 后处理管线
- [docs/MACHINE_CONFIG_GUIDE.md](MACHINE_CONFIG_GUIDE.md) — 引用机床配置
- [templates/turning/demo_gcode.j2](../templates/turning/demo_gcode.j2) — 完整示例
