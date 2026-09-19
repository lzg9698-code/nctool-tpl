# templates/ — 模板目录约定

本目录是 NCtool 的模板根。模板以**相对本目录的路径为注册键**（如
`turning/undercut.j2`），支持任意深度递归发现。

> 编写语法、过滤器清单、常见陷阱见
> [`docs/TEMPLATE_WRITING_GUIDE.md`](../docs/TEMPLATE_WRITING_GUIDE.md)。
> 从外部项目移植模板的方法见
> [`docs/TEMPLATE_INTEGRATION_PLAN.md`](../docs/TEMPLATE_INTEGRATION_PLAN.md) §5.5。

---

## 1. 目录布局

```
templates/
├── templates.yaml              # 模板清单（声明式元数据）
├── README.md                   # 本文件
├── general/                    # 通用子程序（程序头/尾、换刀、安全移动）
├── milling/                    # 铣削
├── turning/                    # 车削
├── grooving/                   # 切槽
├── drilling/                   # 钻孔
└── machines/                   # 机床专用方案包
    └── index_g420/             # 每台机床一个子目录
```

**目录名即默认分类**（可用清单覆盖）：

| 目录 | 分类 |
| --- | --- |
| `general/` | 通用 |
| `milling/` | 铣削 |
| `turning/` | 车削 |
| `grooving/`（或 `groove/`） | 切槽 |
| `drilling/` | 钻孔 |
| `machines/` | 机床 |
| 其它未识别目录 | 通用 |

---

## 2. 元数据的三个来源

优先级 **高 → 低**：

1. **`templates.yaml` 清单条目**（显式声明）
2. **模板头部注释**
3. **文件名 / 目录名**（默认值）

因此清单**只需描述「与默认值不同」的部分**——默认可见、默认后缀 `.NC`、
分类按目录推断的模板可以完全不写清单条目。

### 头部注释写法

```jinja
{# NAME: 越程槽加工（ES 型） -#}
{# DESCRIPTION: ES 型越程槽（DIN509 风格），左右镜像；不含 FS 专用参数 -#}
{# PARAMS:
     side            string  required  加工侧面 ("Right" 或 "Left")
     FS_Z_PLUS1      number  条件必选  FS Z+1 坐标 (mm)，仅 side="Right" 时必选
     STD_KEY         string  必选      标准刀具键值
   #}
```

- `NAME` / `DESCRIPTION` 只扫描文件**前 10 行**；`PARAMS` 是一张表，
  以 200 行为界（机床模板十余个参数，收尾 `#}` 会落到第 10 行之后）。
- `-#}`（右剥空白）用于压掉注释行产生的空行，解析器会正确剥离尾随 `-`。
- **注释正文里不要写 `#}` 字面量**——Jinja 注释在第一个 `#}` 处结束，
  会提前终止注释并把后续内容泄漏成可见文本。

### `{# PARAMS: #}` 参数表 —— **这是会生效的规格**

参数表不是注释装饰：它被解析成 `ParamSpec`，**驱动 `validate` 的类型检查**。
类型写错或漏写会让该参数失去类型保护（见下方"类型列"）。

**一行一个参数，两种写法等价、可混用**：

```jinja
{# PARAMS:
     U_A     必选   键槽有效长度 A (mm)                    # 写法一：名字 + 必选性 + 描述
     side    string  required  加工侧面                    # 写法二：名字 + 类型 + 必选性 + 描述
   #}
```

| 列 | 取值 |
| --- | --- |
| 参数名 | 与模板中变量名一致 |
| 类型（可省） | `number` / `integer` / `string` / `bool` / `list` / `choice` / `any`（亦接受中文：数值/整数/字符串/布尔/列表/枚举/未标注） |
| 必选性 | `必选` / `可选` / `条件必选`（亦接受 `required` / `optional` / `req` / `opt`），可带分支限定词：`可选(ES)` / `必选(FS)` |
| 描述 | 其余内容，内部空白归一化为单空格 |

- **类型列可省**：省略时该参数记为 `any`（**不做类型检查**），但白名单与条件必选
  仍可通过清单声明。省类型是**过渡态**——参数类型应从图纸/源变量库补齐，
  否则 `Z_START=abc` 这类错误要到渲染期才暴露（且渲染期报错不如校验期清楚）。
- **无法解析的行会告警**（`warning: <模板>: {# PARAMS: #} 第 N 行无法解析…`），
  不静默跳过——静默丢一行等于静默少一条参数约束。
- `条件必选` 只表达"不是无条件必选"；**触发条件写不在这里**（头部表达不了），
  要在清单 `params.required_if` 里声明，见下节。

### 清单 `params`：稀疏覆盖层

头部参数表只能表达"名字 + 类型 + 必选性 + 描述"；**约束**（`min` / `max` /
`integer` / `options` / `required_if` / `default`）写在 `templates.yaml` 里。
**只写要改的字段**，其余沿用头部声明：

```yaml
templates:
  "turning/undercut_fs.j2":
    params:
      # 互斥分支参数：只有对应分支可达时才必选
      - name: FS_Z_PLUS1
        required_if: { param: side, values: ["Right"] }
      - name: FS_Z_MINUS1
        required_if: { param: side, values: ["Left"] }

  "machines/index_g420/uz_dj_x.j2":
    params:
      # 有限集合 + 数值类型（防"字符串 0"让 `{% if U_Q == 0 %}` 静默为假）
      - name: U_FX
        kind: choice
        options: ["闭口", "左开口", "右开口"]
      - name: U_Q
        kind: number
        options: [0, 8, 10, 12.5]

  "turning/<你的模板>.j2":
    params:
      # 变量库给 U_Q 限定了 [0, 8, 10, 12.5]，本模板要走别的槽宽 ——
      # 写 null 解除继承来的白名单（**只对模板确实引用了的变量生效**）
      - name: U_Q
        options: null
```

- 可覆盖的字段：`kind` / `required` / `default` / `min` / `max` / `integer` /
  `unit` / `options` / `required_if` / `derive` / `description`。
- **写 `null` 表示「清空」**，即解除从 `variables.yaml` / 头部继承来的那一条：
  `min: null` 去掉下界、`options: null` 去掉白名单、`derive: null` 取消派生
  （`options: []` 与 `options: null` 等价）。**不写**该键才是「沿用继承值」——
  三种写法语义各不相同，别把「没写」当成「清空」。
- 头部没声明的参数也能在这里新增（`kind` 缺省为 `any`）。
- 拼错字段名会**直接报错**（`deny_unknown_fields`），不会静默忽略。
- 清单里写了、但**没有对应模板文件**的键会在加载时告警
  （`warning: 清单条目 … 未匹配到任何模板文件`）—— 这类条目的约束一条都不会生效，
  最常见的原因是路径拼错（`undercut.j2` vs `undercut_fs.j2`）。
- 候选值/触发值直接写裸标量即可（`options: [0, 8, 12.5]`、
  `values: ["Right"]`），不必写 `{type, value}` 带标签形式。

### `variables.yaml`：全局变量库

同一个变量名在多台机床/多个模板上含义一致时，**只写一次**——不必在 12 个模板里
重复声明 `U_Q` 的候选值：

```yaml
# templates/variables.yaml
variables:
  - name: U_Q # 过渡圆角半径 Q (mm)[圆头键槽 U_Q=0]
    kind: number
    options: [0, 8, 10, 12.5]
  - name: U_ID # 键槽位置，直接拼进 I_R9{{ U_ID }}
    kind: string
    options: ["[41]", "[42]", "[43]"]
```

**三级优先级（高 → 低）**：清单 `params`（本模板覆盖）> `variables.yaml`（全局）>
头部 `{# PARAMS: #}`（模板局部）。合并是**按字段稀疏覆盖**。

**生效范围**：只对「模板确实引用了」的变量生效（头部声明的 + 源码里引用到的）。
因此库里可以放心放全局变量——没被引用的不会注入规格。

**三条刻意的不导入**（都为了不产出"跑得通但错误"的 G-code）：

1. **不导入默认值**：源库默认值是针对特定样件的预填值（`U_A = 141.25` 是那根轴的
   长度）。规格默认值会让**缺参静默通过**，用户少填一个零件尺寸就会拿到另一根轴的
   程序。CNC 零件尺寸没有"合理默认值"。
2. **不导入描述**：头部描述是写给这个模板的，更贴近上下文；源库描述以 YAML 注释保留。
3. **`read_only` 变量只导入类型、不导入候选值**：源库的 `read_only` 意为"由机床设置/
   派生计算决定"，其 `options` 是**某次装夹的取值快照**（`U_ANG: [20]`、`R1: [4000]`），
   当作用户可选集会禁掉合法的换刀/换料调整。真正由用户选择的变量（键槽宽度系列、
   槽宽系列、刀号、开口方向、顶尖型号）才导入 `options`。

需要库之外的取值时，在该模板的清单 `params` 里覆盖即可（覆盖是本模板局部的）。

#### `derive`：由 Rust 侧算好的派生参数

查表型的换算**不写在模板里**（项目原则：模板只做变量替换，计算在 Rust 侧完成）。
在变量库里声明规则即可，模板只引用派生后的参数名：

```yaml
  - name: tip_depth # 尾座中心孔深度
    kind: number
    derive:
      from: tip_model # 源参数（查表主键）
      table: # (源值, 派生值)
        - [B4, 8.51]
        - [DM24, 29.61]
      fallback: 29.61 # 源缺失/未命中时的回退值；不写 = 报错
```

模板里只写 `{{ tip_depth }}`。语义：

| 情形 | 行为 |
| --- | --- |
| 派生参数未提供 | **不要求调用方提供**（与 `machine` 同属系统注入值） |
| 调用方提供了派生参数 | **派生值恒胜**，并给一条警告说明输入会被覆盖 |
| 源参数缺失 / 未命中表项 | 有 `fallback` 用它；没有则报 `DeriveFailed`（**不取 0**） |
| 派生结果 | 照常过类型/白名单/区间检查（派生不是绕过校验的后门） |

源参数可以靠规格 `default` 兜底——派生会在查表前先应用一次默认值。

> 为什么不让调用方覆盖？派生值必须与源参数一致：表里说 `DM24 → 29.61`、用户却填 `10`，
> 允许覆盖就等于允许"型号与深度对不上"的 G-code。

### 清单字段

| 字段 | 默认值 | 说明 |
| --- | --- | --- |
| `name` | 头部注释 / 文件名 | 显示名 |
| `description` | 头部注释 / 空串 | 描述 |
| `visible` | `true` | 是否出现在选择列表 |
| `output_filename` | 模板名 | 输出文件名 |
| `output_extension` | `.NC` | 输出后缀（`.MPF` / `.SPF` / …） |
| `category` | 按目录推断 | 分类 |
| `machine` | 无 | 机床标识；声明后仅在该机床被选中时暴露 |
| `status` | `unreviewed` | `unreviewed` / `reviewed` / `verified` |
| `params` | 无 | 参数规格稀疏覆盖层（见上节） |

**未知字段会被拒绝**（`deny_unknown_fields`），拼错字段名会直接报错而非静默忽略。

清单支持两种形式（带 `templates:` 键、或顶层裸映射），两者等价：

```yaml
# 形式一：带键
templates:
  "turning/undercut.j2":
    name: "越程槽加工"

# 形式二：裸映射
"turning/undercut.j2":
  name: "越程槽加工"
```

> YAML 陷阱：双引号里 `\u` 是 Unicode 转义。
> 键含反斜杠时用**单引号**（`'turning\undercut.j2'`）或干脆用正斜杠。

---

## 3. 两种「隐藏」的语义

两者都靠 `visible: false` 实现，**含义不同**：

| 类型 | 命名 | 含义 | 例 |
| --- | --- | --- | --- |
| 完整但不上架 | 正常命名 | 可单独渲染，只是不出现在选择列表（由其它程序调用） | `grooving/circlip_groove.j2`、`machines/*/dg_cal_ir9.j2` |
| 内部片段 | **下划线前缀** | **不完整、不可单独渲染**，仅供 `include` | `turning/_undercut_common.j2` |

内部片段必须用下划线前缀命名，这是约定而非强制检查——但
`templates list --all` 会同时列出两类，靠前缀区分。

---

## 4. `{% include %}` 必须写全路径

模板以相对本目录的路径为注册键：

```jinja
{% include "turning/_undercut_common.j2" %}   {# ✅ #}
{% include "_undercut_common.j2" %}           {# ❌ TemplateNotFound #}
```

`nctool inspect` 会穿透 `include` 把片段引用的参数并入参数表，
并在输出末尾提示行列号指向片段文件自身。

---

## 5. 机床方案包（`machines/`）

每台机床一个子目录，专用模板必须声明 `machine`：

```yaml
"machines/index_g420/uz_fkm_temp1.j2":
  name: "精铣：闭口非圆头键槽"
  output_filename: "UZ_FKM_temp1"
  output_extension: ".SPF"
  machine: index_g420
  status: unreviewed
```

查看某台机床的全部模板：

```bash
nctool templates list --machine index_g420 --all
```

未选中该机床时，这些模板**不可见**（避免通用选择列表被机床专用模板淹没）。

### 现有方案包

| 机床 | 模板数 | 说明 |
| --- | --- | --- |
| `index_g420` | 19 | 主程序（`.MPF`）× 6、R 参数初始化 × 1、键槽倒角 × 1、精铣 × 4、粗铣 × 7（`.SPF`） |

> 机床模板均标记 `status: unreviewed`——迁移自 NCTool_V3，**未经工艺评审**，
> 上机前必须复核。

---

## 6. 常用命令

```bash
# 列出全部模板（含隐藏，· 标记为隐藏项）
nctool templates list --template-dir ./templates --all

# 只看某台机床的模板
nctool templates list --template-dir ./templates --machine index_g420 --all

# 查看单个模板的参数表（穿透 include）
nctool inspect turning/undercut_es.j2 --template-dir ./templates

# 渲染前校验
nctool validate turning/undercut_es.j2 --template-dir ./templates --params-file p.json

# 渲染
nctool render turning/undercut_es.j2 --template-dir ./templates --params-file p.json
```

> `--template-dir` 的相对路径**基于当前工作目录**。若在仓库根执行，
> `--template-dir ./templates` 即可；不传则读 `nctool.toml` 中的配置
> （`nctool config show` 可确认）。
