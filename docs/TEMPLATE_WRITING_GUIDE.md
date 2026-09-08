# 模板编写指南

> 适用版本：nctool-tpl ≥ 0.3.2
> 模板基于 [Jinja2](https://jinja.palletsprojects.com/)（用 [minijinja](https://github.com/mitsuhiko/minijinja) 引擎）。
> 本指南聚焦**面向 NC/G-code 的实战写法**与本项目独有的约定，**通用 Jinja 语法**请参考上游文档。

## 1. 第一个模板

`templates/demo_gcode.j2` 是一个完整示例，可直接 `cargo run --example demo` 跑通。

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

G-code 对数值格式敏感，库内置三个专用过滤器。

| 过滤器 | 用法 | 输入 | 输出 | 用途 |
| --- | --- | --- | --- | --- |
| `nc_fixed(N)` | `{{ x \| nc_fixed(3) }}` | `21.0` | `21.000` | 固定小数位的坐标值 |
| `nc_strip` | `{{ x \| nc_strip }}` | `21.0` | `21` | 去尾零，避免 `X21.0`（部分控制器不接） |
| `nc_pad(N)` | `{{ n \| nc_pad(4) }}` | `1` | `0001` | 程序号/行号前导零 |

所有 NC 过滤器对 **NaN/Inf 报错**（防止非法数值写入 G-code）。该防线仅覆盖本库注册的过滤器；裸 `{{ x }}` 输出或 minijinja 内建操作（如 `round`、算术）产生的 NaN/Inf 不经此校验——先用 `nctool validate` 拦。

边界与坑：

- `nc_pad(宽度)` 上限 **1024**；`nc_fixed(小数位)` 上限 **32**。超出即报错（避免巨量分配导致 abort）
- `nc_pad` 输入必须是整数或整值的 `Number`（`5.0` 行，`5.5` 不行）—— 否则报错

## 4. 数学过滤器

全部基于 `f64` 标准库，零额外依赖：

`sin` `cos` `tan` `asin` `acos` `atan` `sqrt` `exp` `ln` `log10` `pow` `floor` `ceil`

```jinja
G1 X{{ x | default(0) }} Y{{ (r * sin(theta * pi / 180)) | nc_fixed(4) }}
```

**有限性校验**：所有数学过滤器对结果做 `is_finite()` 检查；`sqrt(-1)`、`ln(0)` 立即报错，避免非法坐标静默写入 G-code。

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
- [templates/demo_gcode.j2](../templates/demo_gcode.j2) — 完整示例
