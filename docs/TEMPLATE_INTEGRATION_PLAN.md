# NCTool_V3 模板资产整合方案

> 目标：把 `NCTool_V3`（Python + Jinja2）中沉淀的模板资产、配置约定与组件抽象，
> 整合为 `nctool-tpl`（Rust + minijinja）当前项目的**模板基础**。
>
> 分析对象：
> - 源仓库 `https://github.com/lzg9698-code/NCTool_V3.git`（Python 3.9+ / CustomTkinter / Jinja2 / Nuitka）
> - 目标仓库 `nctool-tpl`（Rust 1.85+ / minijinja / workspace: `nctool-tpl` + `nctool-core` + `nctool-cli`）
>
> 版本：v1.0 · 2026-09-12

---

## 1. 结论先行（TL;DR）

| 判断 | 内容 |
| --- | --- |
| **能直接搬的** | 22 个 `.j2` 模板文件、`templates.yaml` 清单约定、`{# NAME: #}` 头部元数据约定、变量库 schema、输出文件名/后缀规则、ES/FS 刀具标准数据库 |
| **能搬思路但不能搬代码的** | 分层架构（UI / Service / Logic / Engine）、`VariableRepository` 的惰性加载与联动映射、模板哈希缓存、错误异常层次 |
| **必须重写的** | 全部 Python 实现（Python→Rust）、CustomTkinter UI（→ 现有 `ui/index.html` Web UI）、`sys.frozen` 打包路径逻辑 |
| **当前项目最大的两个缺口** | ① 模板注册是**硬编码在 Rust 源码里**的（`core/src/registry.rs::builtin_templates()`），无外部模板发现能力；② 模板目录扫描**不递归**（`cli/src/context.rs` 用 `read_dir`，只认平铺的 `*.j2`） |
| **整合核心难点** | 源模板的**数学内联在模板里**（用 `{% set %}` 做三角/几何计算），而目标项目的设计是**由 Rust 侧预计算后注入上下文**。二者需要统一到一条边界线上 |

**一句话方案**：把源仓库的模板当作「**内容资产**」而非「**代码资产**」——模板文件与配置清单整体迁入，
在 Rust 侧新建 `TemplateSource` 抽象与递归目录发现 + `templates.yaml` 清单加载，
并把源模板中内联的数学计算**上移到 Rust 的生成器层**（新增 `nctool-derive` 或 `core/src/derive/`），
保持「模板只做变量替换」这一目标项目已有的正确边界。

---

## 2. 源项目（NCTool_V3）结构分析

### 2.1 目录布局

```
NCTool_V3/
├── unified_nc_app.pyw          # 入口：CTk 主窗 + 5 个 Tab + 状态栏（424 行）
├── build.py                    # Nuitka 打包脚本
├── installer_script.iss        # Inno Setup 安装器
├── pyproject.toml / ruff.toml / pytest.ini
├── core/                       # 业务逻辑层（约 1900 行）
│   ├── template_engine.py      # ★ TemplateParser：Jinja2 环境 + 变量提取 + 哈希缓存（385 行）
│   ├── template_service.py     # ★ TemplateService：协调 Parser 与 Repo（91 行）
│   ├── variable_manager.py     # ★ VariableRepository：变量定义/类型/校验/联动（370 行）
│   ├── config_manager.py       # 静态配置读写（59 行）
│   ├── loaders.py              # ★ ConfigLoader：类型安全加载（88 行）
│   ├── interfaces.py           # ★ Protocol 接口契约（112 行）
│   ├── exceptions.py           # ★ 异常层次（102 行）
│   ├── error_handler.py        # 日志 + 用户友好提示（186 行）
│   ├── error_messages.py       # 错误码字典（99 行）
│   ├── models.py               # ★ dataclass 数据模型（44 行）
│   ├── utils.py                # ★ 路径解析 / 配色 / 数值格式化（157 行）
│   ├── undercut_logic.py       # 越程槽：ES/FS 刀具库 + 几何计算（253 行）
│   ├── sync_turning.py         # 同步车削：左右侧刀次计算（265 行）
│   └── circlip_groove_logic.py # 卡簧槽（93 行）
├── ui/                         # 表现层（CustomTkinter，约 3300 行）
│   ├── base_frame.py           # ★ BaseNCFrame：卡片/日志/安全取数/剪贴板（131 行）
│   ├── widgets.py              # ★ DynamicInputFrame / GCodeTextbox / StepTable（903 行）
│   ├── variable_editor.py      # 变量库编辑器（573 行）
│   ├── template_gen_frame.py   # 模板生成 Tab（602 行）
│   ├── undercut_frame.py       # 越程槽 Tab（506 行）
│   ├── sync_turning_frame.py   # 同步车 Tab（734 行）
│   ├── circlip_groove_frame.py # 卡簧槽 Tab（397 行）
│   └── handwritten_frame.py    # 手写轨迹 Tab（151 行）
├── templates/                  # ★ 22 个 .j2 模板
│   ├── undercut_template.j2
│   ├── circlip_groove_template.j2
│   ├── sync_turning_left_template.j2
│   ├── sync_turning_right_template.j2
│   └── INDEX G420/             # 18 个机床专用模板（.MPF / .SPF）
├── configs/                    # ★ 配置
│   ├── templates.yaml          # 模板清单（可见性 / 输出名 / 后缀）
│   ├── variable_repo.json      # 变量库（62 个变量）
│   ├── undercut_config.json    # 越程槽参数
│   ├── turning_config.json     # 同步车参数
│   └── circlip_groove_config.json
├── tests/                      # 10 个 pytest 模块
└── docs/                       # 架构 / 规范 / 变更日志
```

### 2.2 分层与依赖方向

```
unified_nc_app.pyw
   │
   ├──> ui/          表现层 ──┐
   │                         │  只依赖 core 的抽象（Protocol）
   └──> core/        逻辑层 <┘
          ├── template_service.py   ← 服务编排
          │      ├── template_engine.py
          │      └── variable_manager.py
          ├── undercut_logic.py / sync_turning.py / circlip_groove_logic.py
          ├── exceptions.py / error_handler.py
          └── interfaces.py  (Protocol：ErrorCallback / TemplateLoader / VariableStorage)
```

依赖方向**单向向下**，`core/` 不反向依赖 `ui/`；`interfaces.py` 用 `Protocol` 打破循环依赖。
这套结构与本项目现有的 `nctool-tpl`（引擎）/ `nctool-core`（编排）/ `nctool-cli`（表现）**高度同构**，
因此**架构无需改造，只需把资产内容填进去**。

### 2.3 设计模式对照

| 源项目模式 | 实现类 | 目标项目对应物 | 迁移动作 |
| --- | --- | --- | --- |
| Repository | `VariableRepository` | 无（`ParamSpec` 硬编码在模板定义里） | **新增**变量库外部化能力 |
| Service | `TemplateService` | `GCodeGenerator` | 已具备，需补清单同步 |
| 模板方法 | `BaseNCFrame` | 无（Web UI 走服务端） | 概念迁移到 HTML/CSS 组件 |
| Protocol | `ErrorCallback` / `TemplateLoader` | Rust trait（隐式） | 已具备 |
| 缓存 | 模板哈希缓存 | `Renderer` 内部编译缓存 | 已具备 |

---

## 3. 模板类型分类（核心分析）

这是本次整合的**核心产出**。22 个模板按「参数规模 × 内联计算量 × 复用方式」可分为 4 类，
对应 4 种完全不同的迁移与治理策略。

### 3.1 分类总表

| 类别 | 模板 | 数量 | 变量数 | 行数 | 内联计算 | 复用性 | 迁移策略 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **A. 原子子程序** | `circlip_groove_template.j2` | 1 | 6 | 21 | 无 | 高 | 直接移植 |
| **B. 通用参数化模板** | `undercut_template.j2` | 1 | 1* | 119 | 无（已外移） | 高 | 直接移植 |
| **C. 计算重型模板** | `sync_turning_left/right_template.j2` | 2 | 1–2* | 33–46 | **高**（循环内几何） | 中 | 计算上移到 Rust |
| **D. 机床专用程序** | `INDEX G420/*.j2` | 18 | 1–10 | 49–452 | **极高**（三角/条件分支） | 低（机床锁定） | 原样归档 + 元数据 |

> \* 变量数低是因为这些模板接收的是**已计算好的派生参数**（如 `R11`/`R22`/`z_base`），
> 真实输入参数在 Python 侧计算后注入。详见 §3.4 的「计算边界」。

### 3.2 类别 A — 原子子程序（可直接移植）

**代表**：`templates/circlip_groove_template.j2`

```jinja
{# 卡簧槽程序模板 #}
{% for z_pos in z_offsets %}
; --- 卡簧槽 Z={{ z_pos | fmt_coord }} ---
G0 Z{{ z_pos | fmt_val }}
G0 X{{ (X_RAW_D + 10) | fmt_val }}
M4=97
R12={{ X_TIP_D | fmt_val }}
R15={{ (X_TIP_D - X_DOWN_D) | fmt_val }}/2
R18={{ MAX_CUTDIPTH | fmt_val }}
DB_CYCLE93(R12,R13,R14,R15,R16,R17,R18,R19,R20,R21)
G0 X{{ (X_RAW_D + 10) | fmt_val }}
STOPRE
{% endfor %}
```

**特征**：单一工序、循环驱动、零算术（只有简单减除）、参数扁平。
**适用场景**：作为其他模板的 `{% include %}` 子程序，或直接被 CLI `render` 调用。
**移植成本**：极低。仅需把 `fmt_coord` / `fmt_val` 两个过滤器映射到本项目已有的
`nc_fixed(n)` / `nc_strip`。

### 3.3 类别 B — 通用参数化模板（可直接移植）

**代表**：`templates/undercut_template.j2`

**关键设计优点**：模板**头部有完整的参数文档表**（注释形式），明确列出
「必需输入变量 / ES 类型特有变量 / FS 类型特有变量 / 计算规则」，而**所有中间量已在 Python 侧算好**：

```jinja
{# 参数文档表:============================
    必需输入变量:
    - groove_type: 工艺类型 ("ES" 或 "FS")
    - side: 侧面 ("Right" 或 "Left")
    ...
    计算规则:
    ES类型计算规则:
    1. 从数据库获取参数: ES_r, ES_t1, ES_f, ES_ang
    ...
#}
```

**这是整个源仓库最值得学习的约定**：模板自带参数契约文档，读者无需翻 Python 代码即可知道要传什么。
**适用场景**：单一工序 + 双工艺分支（ES/FS）+ 多标准刀具可选。
**移植成本**：低。**建议在本项目强制推行这一「模板头部参数文档表」约定**（见 §6.2 约定 R2）。

### 3.4 类别 C — 计算重型模板（需重构）

**代表**：`sync_turning_left_template.j2`

```jinja
{% for i in range(1, cut_num + 1) %}
{% set x_val_ch1 = actual_sta_x - ap_1 - (i - 1) * (ap_1 + ap_2) %}
{% set x_val_ch2 = actual_sta_x - i * (ap_1 + ap_2) %}
{% set z_base = actual_sta_z - (i-1) * (ap_1+ap_2)/(2*tan(25)) %}
{% set z_start_ch1 = z_base + dis_1/2 %}
G1 Z{{ "%.3f"|format(z_base) }} ANG=225 F0.2
{% endfor %}
```

**问题**：几何计算**内联在模板的 for 循环里**，模板承担了算法职责。带来三个具体风险：
1. **不可单测**——算法正确性只能靠渲染结果倒推；
2. **不可复用**——同一几何逻辑在 left/right 两个模板中重复；
3. **误差不可控**——`tan(25)` 是硬编码字面量，缺乏参数化与校验。

**与本项目的冲突**：目标项目的设计原则是「**模板只做变量替换，计算在 Rust 侧完成**」
（见 `core/src/machine.rs` 与 `core/src/pipeline.rs` 的注释）。
`sync_turning` 的做法与之相悖，**必须重构**。

**改造方案**：把 for 循环内的计算抽到 Rust 侧生成器，模板退化为遍历已算好的点数组：

```jinja
{# 改造后：模板只负责格式化输出 #}
{% for pt in cuts %}
G0 Z{{ pt.z_approach | nc_fixed(3) }}
G0 X{{ pt.x_approach | nc_fixed(3) }}
WAITM({{ pt.sync_num }},1,2)
G1 Z{{ pt.z_cut | nc_fixed(3) }} ANG=225 F{{ pt.feed | nc_fixed(1) }}
G1 X{{ pt.x_cut | nc_fixed(3) }} ANG=205 F{{ pt.feed | nc_fixed(1) }}
{% endfor %}
```

数值由 Rust 的 `derive::sync_turning::plan_cuts(...)` 产出。
**适用场景**：多刀次循环 + 同步通道编号 + 左右侧镜像。

### 3.5 类别 D — 机床专用程序（归档 + 元数据）

**代表**：`templates/INDEX G420/*.j2`（18 个）

命名规律清晰，可归纳为 4 组：

| 组 | 文件 | 名称模式 | 说明 |
| --- | --- | --- | --- |
| **主程序** | `1_0.MPF_A/B` … `3_0.MPF_A/B` | `{序号}_0.MPF_{A\|B}` | 同步车 + 键槽 + 越程槽 + 精车，201–452 行 |
| **全局参数** | `DG_CAL_IR9.SPF` | — | 全局参数配置，7 变量 49 行 |
| **倒角** | `UZ_DJ_X.j2` | `UZ_DJ_X` | 30 键槽倒角，10 变量 143 行 |
| **键槽铣削** | `UZ_FKM_temp1–4` / `UZ_RKM_temp1–7` | `UZ_{F\|R}KM_temp{N}` | 精铣 4 个 + 粗铣 7 个 |

其中 `UZ_RKM_temp*` 的命名编码了**工艺决策矩阵**，信息量极高：

```
11-闭口-非圆头键槽-粗铣(深度1刀/宽度2刀/刀具半径=圆角半径)
12-闭口-非圆头键槽-粗铣(深度1刀/宽度3刀/刀具半径<圆角半径)
13-闭口-非圆头键槽-粗铣(深度1刀/宽度3刀/刀具半径=圆角半径)
14-闭口-非圆头键槽-粗铣(深度2刀/宽度3刀/刀具半径<圆角半径)
15-右开口-非圆头键槽-粗铣(深度1刀/宽度2刀/刀具半径=圆角半径)
16-闭口-圆头键槽-粗铣(深度1刀/宽度2刀/刀具半径<圆角半径)
17-左开口-非圆头键槽-粗铣(深度1刀/宽度2刀/刀具半径=圆角半径)
```

三个决策维度：**开口形式**（闭口/左开口/右开口）× **底部形式**（圆头/非圆头）
× **刀路数**（深度刀数/宽度刀数/刀具半径与圆角半径关系）。

**特征**：
- 内含大量 `{% if U_FX == "左开口" %}` 式的**工艺分支**；
- 三角函数密集（`(U_D/2)**2 - (U_B/2)**2)**0.5`）；
- 依赖机床专有 G 代码（`L184` / `AROT` / `ATRANS` / `SETMS` / `M4=97` 等 INDEX 扩展）；
- 变量数极低（1–10）但行数极高（最高 452 行）——**说明是「固化工艺」而非「参数化模板」**。

**适用场景与策略**：这类模板**不适合做通用模板库**，应作为「**机床方案包（Machine Recipe Pack）**」
归档：整目录保留，通过清单声明为 `visible: false`（源项目正是这么做的），
仅在用户显式选择 `index_g420` 机床时暴露。

---

## 4. 配置方式分析

源项目用**三套互补的配置机制**，各自解决不同问题。这是整合中最需要仔细对待的部分。

### 4.1 三套配置机制

#### 机制一：`templates.yaml` — 模板清单（声明式）

```yaml
templates:
  "INDEX G420/1_0.MPF_A.j2":
    name: "INDEX G420 1_0_A"
    visible: true
    output_filename: "1_0"
    output_extension: ".MPF"

  circlip_groove_template.j2:
    name: "卡簧槽模板"
    visible: false          # 功能模块专用 → 隐藏
    output_extension: ".NC"
```

**字段语义**：

| 字段 | 作用 | 缺省 |
| --- | --- | --- |
| `name` | 显示名 | 回退到模板头部 `{# NAME: #}`，再回退到文件名 |
| `description` | 描述 | 回退到 `{# DESCRIPTION: #}` |
| `visible` | 是否在模板列表中可见 | `true` |
| `output_filename` | 输出文件名（支持 Jinja 变量） | 空（用模板名） |
| `output_extension` | 输出后缀（`.NC` / `.MPF` / `.SPF`） | `.NC` |

**关键设计**：`visible: false` 用来把「**功能模块专用模板**」从用户选择列表中藏起来，
但保留其可被程序调用。这是一种**面向用户的视图过滤**，与模板的真实可用性解耦。

#### 机制二：模板头部元数据 — 就近声明

```jinja
{# NAME: 21-闭口-非圆头键槽-精铣 #}
{# DESCRIPTION: 闭口非圆头键槽精铣工序，适用于 ... #}
```

提取逻辑硬编码为「检查前 10 行、按 `{# NAME:` / `{# DESCRIPTION:` 字符串切分」。
**优点**：模板与元数据同文件，移动/复制不丢失。
**缺点**：手写式实现脆弱（依赖固定格式与前 10 行限制）。

#### 机制三：`variable_repo.json` — 变量库（62 个变量）

这是源项目**最有价值的可复用设计**。schema 如下：

```jsonc
{
  "U_A": {
    "description": "键槽有效长度 A (mm)",
    "data_type": "float",          // float | int | str | bool | choice
    "default_value": 141.25,
    "min_value": 10.0,             // 数值范围校验
    "max_value": 500.0
  },
  "U_B": {
    "description": "键槽成活宽度 B (mm)",
    "data_type": "choice",
    "default_value": 25,
    "bound_area": "无绑定",        // 关联到 UI 的哪个预览区
    "options": ["16","18","22","25","28","32","36","40","45","50"],
    "value_type": "int"            // choice 的底层类型
  },
  "U_ANG": {
    "description": "粗铣入刀角度 (°)",
    "data_type": "choice",
    "default_value": 20,
    "read_only": true,             // 只读（被联动写入）
    "options": ["20"],
    "value_type": "int"
  },
  "U_RT": {
    "description": "粗铣刀具名称",
    "data_type": "choice",
    "default_value": "CT1017",
    "associations": {              // ★ 联动映射
      "CT1017": { "R1": 4000, "R6": 40 },
      "CT1020": { "R1": 3200, "R6": 50 }
    }
  }
}
```

**完整字段清单**：`description` / `data_type` / `default_value` / `min_value` / `max_value`
/ `options` / `value_type` / `read_only` / `bound_area` / `associations`

**实测分布**（62 个变量）：

| 维度 | 分布 |
| --- | --- |
| `data_type` | `choice` 33 · `float` 16 · `str` 11 · `int` 2 |
| `bound_area` | 无绑定 54 · `同步车-左/右侧Ch1/Ch2` 4 · 左/右侧越程槽NC 2 · 卡簧槽程序-代码预览 1 · 手写轨迹-左侧粗车 1 |
| 含 `associations` | 2 个（`U_RT`、`U_FT`） |

两个设计亮点：

1. **`associations` 联动映射** — 选一个刀具名，自动带出转速上限、伸出长度等**一组参数**。
   这是「刀具库」概念的最小可用实现。
2. **`read_only` + `associations` 组合** — 被联动写入的变量标 `read_only`，UI 渲染为不可编辑，
   但联动逻辑可程序化写入。避免了「用户手改被覆盖」或「用户乱改导致不一致」两类问题。

### 4.2 与本项目现有配置机制的对照

| 能力 | NCTool_V3 | nctool-tpl（现状） | 差距 |
| --- | --- | --- | --- |
| 模板清单声明 | `templates.yaml` | **无**（硬编码在 `registry.rs`） | ✅ 已补（阶段 1） |
| 模板元数据 | 头部 `{# NAME: #}` | 无 | ✅ 已补（阶段 1） |
| 可见性过滤 | `visible: false` | 无 | ✅ 已补（阶段 1） |
| 输出命名规则 | `output_filename` / `output_extension` | 无（CLI `-o` 手动） | ✅ 已补（阶段 1） |
| 递归目录发现 | `rglob("*.j2")` | `read_dir`（**不递归**） | ✅ 已补（阶段 1） |
| 参数规格 | `variable_repo.json` | `ParamSpec`（硬编码在 `builtin_templates()`） | **需外部化** |
| 参数类型 | 5 种（含 `choice`） | `Number`/`Integer`/`String`/`Bool` | 缺 `Choice`（阶段 3） |
| **列表参数** | 靠 Python 侧循环 | **无列表类型** | ✅ 已补（`ParamValue::List`） |
| 参数范围校验 | `min_value` / `max_value` | `.with_range(min, max)` | 已具备 |
| 参数联动 | `associations` | 无 | 需新增（阶段 3） |
| 只读参数 | `read_only` | 无 | 需新增（阶段 3） |
| 机床配置 | 隐含在模板中 | `MachineConfig` + 3 个预设 | **本项目更强** |
| 配置合并 | `deep_merge_dict` | `nctool.toml` 层叠 | 本项目更强 |

**重要结论**：本项目在**机床配置**与**配置层叠**上明显优于源项目，
但在**模板元数据管理**与**参数库外部化**上明显落后。
整合的方向应是**补短板、不丢长板**。

### 4.4 实施中发现的能力缺口（阶段 1–2 实测，共七个）

下面七个问题在纸面分析阶段无法预见，都是**动手移植模板时才暴露**的。
其中缺口一~三是引擎/参数模型层面的能力边界，缺口四~七是**静默失败类**
缺陷——它们不报错、不崩溃，只是悄悄给出错误结果，因此优先级最高：

#### 缺口一：参数系统没有列表类型（已修复）

源项目的批量工序模板（`circlip_groove` 遍历 `z_offsets`、`sync_turning` 遍历刀次）
天然需要**列表参数**，而本项目的 `ParamValue` 原本只有
`Number` / `Integer` / `String` / `Bool` 四种——**无法表达列表**。

若不修复，移植来的循环驱动模板只能靠调用方逐次渲染再拼接，
既无法在单次调用中完成，也无法整体校验。

**修复**：新增 `ParamValue::List(Vec<ParamValue>)` 与 `ParamKind::List`，
并让 `parameter_set_from_json` 递归解析 JSON 数组。

设计取舍：`ParamKind::List` **只校验"是不是列表"，不校验元素类型**。
原因是元素类型由模板自身隐式约定（`{% for p in passes %}{{ p.x }}` 要求元素有 `x` 字段），
静态声明元素类型会与模板约定重复且容易漂移。需要元素级校验时应在模板内用
`{% if %}` 显式表达。

#### 缺口二：互斥分支参数全部报"必选缺失"（已用「拆分模板」方案解决）

`undercut.j2` 有 ES/FS 两条互斥分支：

```jinja
{% if groove_type == "ES" and side == "Right" %}
G1 Z{{ ES_Z | nc_fixed(3) }}          {# ES 分支用 ES_Z / ES_D2 / D2_COMP_CUT #}
{% elif groove_type == "FS" and side == "Right" %}
G1 Z{{ Z_MID | nc_fixed(3) }}          {# FS 分支用 Z_MID / FS_Z_PLUS1 / ANG_2 #}
{% endif %}
```

而变量提取是**静态**的——它不看 `groove_type` 的运行期取值，
把 ES 与 FS 分支引用的变量**全部**视为必选。实测结果：

```
$ nctool render turning/undercut.j2 --params-file <仅 ES 参数>
error: 错误 [Z_MID] 必选参数缺失（第 54 行第 7 列引用）
错误 [FS_Z_PLUS1] 必选参数缺失（第 55 行第 7 列引用）
错误 [ANG_2] 必选参数缺失（第 55 行第 42 列引用）
错误 [FS_Z_MINUS1] 必选参数缺失（第 66 行第 7 列引用）
```

**这是真问题**：用户明明只做 ES 型越程槽，却被迫填 4 个 FS 专用参数。
源项目的 Python 版没有这个毛病——它靠 `GrooveGenerator.generate()` 在
**调用前**就按 `gtype` 分支准备参数字典，不可达分支的参数根本不会进入渲染上下文。

**可选对策**（阶段 3 一并处理，按推荐度排序）：

| # | 方案 | 优点 | 缺点 |
| --- | --- | --- | --- |
| 1 | **模板内 `default` 兜底**：`{{ Z_MID \| default(0) \| nc_fixed(3) }}` | 无需改引擎；模板自解释 | 兜底值会静默产出错误 G-code（`Z0`）→ **有撞刀风险** |
| 2 | **拆分为两个模板**：`undercut_es.j2` + `undercut_fs.j2` | 语义最清晰；参数表干净 | 模板数量翻倍；公共段落需 `include` 抽取 |
| 3 | **引擎支持条件必选**：在 `ParamSpec` 增加 `required_if: {param: value}` | 一处声明、全局受益 | 需改变量提取器，成本最高 |

**推荐方案 2 + 保留单模板**：`undercut.j2` 作为「两者都填」的完整版保留
（内部/批量生成场景），同时派生 `undercut_es.j2` / `undercut_fs.j2`
两个精简版（用户手动填参数场景）。这样既满足自动化的完整性，
又避免人工填表的困惑，且**不动引擎**。

> 注：方案 1 因存在静默产出错误 G-code 的风险而**不推荐**——
> 与本项目「渲染前可发现错误」的核心设计原则相悖。

**✅ 已按方案 2 实施完成**（下方为实测结论）：

| 产物 | 行数 | 说明 |
| --- | --- | --- |
| `turning/undercut.j2` | 69 | 完整版保留（ES+FS 四分支） |
| `turning/undercut_es.j2` | 27 | ES 精简版，**11 个必选参数** |
| `turning/undercut_fs.j2` | 33 | FS 精简版，**12 个必选参数** |
| `turning/_undercut_common.j2` | 21 | 公共起始段（`visible: false`） |

公共段用 `{% include "turning/_undercut_common.j2" %}` 抽取
（**必须写相对模板根的完整路径**——模板以相对路径键注册，写裸文件名会
`TemplateNotFound`）。补偿方向 `G42/G41` 与收尾角度 `135/45` 都用
`set` + 内联条件收纳，避免在每个分支重复三分支判断。

验证方式为**逐字节比对**：`undercut_es.j2` / `undercut_fs.j2` 在
Right/Left 共 4 种组合下，输出的 G-code 与完整版 `undercut.j2`
**完全一致**（剔除注释行后 `diff` 为空）。

**顺带发现并修复了配套缺口**：拆分后 `inspect` 只列出主模板的变量，
遗漏了 `include` 片段引用的 5 个参数（`STD_KEY` / `Z_START` /
`D1_CUT` / `ANG_1` / `D1_COMP_START`）——用户按参数表填参会在渲染时
才报错。根因是 `cli/src/commands/inspect.rs` 直接调 `extract_undeclared`，
而 `nctool-core` 里已有的 `collect_include_closure`（穿透 include 的
闭包收集）是私有方法、仅服务于 `validate`。

修复：在 `TemplateRegistry` 上新增公开方法 `extract_params(name)`，
把闭包收集的中间结果暴露出来（同时剔除 `machine` 等系统注入变量），
`inspect` 改用它；并在输出末尾提示"本模板引用了 XX 片段，行列号指向
片段文件自身"——因为片段的行号在主模板看来是"越界"的，不说明会让人
以为是 bug。补了 4 个回归测试：穿透 include、剔除系统变量、
未找到模板报错、**环引用不栈溢出**。

#### 缺口三：minijinja 的 map 不支持 `.get()` 等方法调用（已定位，已给出约定）

移植机床模板 `DG_CAL_IR9.SPF.j2` 时暴露。该模板用**字典字面量**做型号查表：

```jinja
{% set tip_depth_map = {'B4': 8.51, 'DM24': 29.61, ...} %}
{% set tip_depth = tip_depth_map.get(tip_model, 29.61) %}
```

这在 Python Jinja2 下正常，但在 minijinja 下直接渲染失败：

```
error: machines/index_g420/dg_cal_ir9.j2: 渲染错误:
       unknown method: map has no method named get (in ...:28)
```

**实测确认的 minijinja map 能力边界**（逐条验证，非推测）：

| 写法 | 结果 | 说明 |
| --- | --- | --- |
| `m.keys()` | ❌ `unknown method: map has no method named keys` | 无任何 map 方法 |
| `m.items()` | ❌ 同上 | |
| `m.get(k, d)` | ❌ `unknown method: map has no method named get` | 这是最常见的中招点 |
| `m['k']`（键存在） | ✅ 正常返回值 | 下标访问可用 |
| `m['k']`（键缺失） | ⚠️ 返回 undefined → Strict 模式下**报错** | 不能裸用 |
| `m['k'] \| default(d)` | ✅ **完全等价于 `dict.get(k, d)`** | ← 标准写法 |
| `m[key_var]`（动态键） | ✅ 正常 | 支持变量作键 |
| `\{'k': v\}` 字典字面量 | ✅ 正常 | 建字典没问题，只是不能调方法 |

**结论**：minijinja 的 map **只有下标访问，没有任何方法**。
`dict.get(k, d)` 的唯一等价写法是 `m[k] | default(d)`
（下标缺失返回 undefined，再由 `default` 兜底）——语义完全一致。

**已固化为约定 R6**，并已在 `dg_cal_ir9.j2` 中采用：

```jinja
{# 约定 R6：minijinja 的 map 不支持 .get() / .keys() 等方法调用，
   查表统一写作 `map[key] | default(回退值)` #}
{% set tip_depth = tip_depth_map[tip_model|default('DM24')]|default(29.61) %}
```

实测三种输入全部正确：显式型号 `DM24 → 29.61`、`B6.3 → 12.82`、
未给型号 → 回退 `29.61`。

> **迁移检查项**：源项目若有其它模板用了 `.get()` / `.keys()` / `.items()` /
> `.values()`，移植时必须逐处改写。建议在阶段 2 用
> `grep -nE '\.(get|keys|values|items)\(' templates/**/*.j2` 扫一遍。

#### 缺口四：清单「裸映射」写法被静默忽略（已修复）

`templates.yaml` 设计上支持两种手写形式：

```yaml
# 形式一（推荐）：带 templates 键
templates:
  "turning/undercut.j2":
    name: 越程槽加工

# 形式二：顶层直接是路径键（裸映射）
"turning/undercut.j2":
  name: 越程槽加工
```

原实现是「先试形式一，失败再试形式二」——**但这个回退永远不会触发**。
原因是 `ManifestFile.templates` 字段带 `#[serde(default)]`：

- 把**裸映射**喂给 `ManifestFile` **不会失败**——所有顶层键被当作未知字段
  忽略，得到一个**空清单**（`templates` 取默认值）
- 于是「形式一成功」这一分支走通，形式二的分支永远轮不到

**后果是静默失效**：用户在清单里写了一堆条目，程序却当作「没有清单」，
所有字段（名称、可见性、输出后缀、机床）全部退回默认值，
**不报任何错**。这类「配置写了但不生效」比直接报错难排查得多。

**修复**：改为先解析成通用的 `serde_yaml::Value`，**显式检查顶层有无
`templates` 键**来决定走哪条路；同时补上边界处理——空文件（解析为 `Null`）
视作「无清单声明」而非错误，顶层为标量/序列时明确报错。

**通用教训**：用「try A / 失败 / try B」来区分输入格式，只在 A 对 B 的输入
**必然失败**时成立。若 A 有 `#[serde(default)]`、`Option` 或其它可选/宽容字段，
A 会「成功」地吞掉 B 的输入并产出语义错误的结果。

补 5 个回归测试：裸映射不静默为空、空文件、纯注释文件、顶层非映射报错、
带 `templates` 键但内容不合法时报错。另修一个测试自身的错误——YAML 双引号里
`\u` 是 Unicode 转义，`"turning\undercut.j2"` 直接解析失败（应用单引号
`'turning\undercut.j2'`，单引号内反斜杠是字面量）。

#### 缺口五：仓库根 `nctool.toml` 污染 E2E 测试（已规避）

项目配置是**从 cwd 向上递归查找到文件系统根**（`find_project_config`）。
为手动验证方便，在仓库根执行 `nctool config init` 得到的 `nctool.toml`
会被 `cli/tests/cli.rs` 中那些**不设 `current_dir`** 的用例读到
（它们在仓库根执行），于是 `template_dir = "templates"` 生效，
而临时目录下并没有 `templates/`，报：

```
error: 模板目录不存在: templates
```

一次导致 **19 个用例失败**。`cli/tests/cli_e2e.rs` 因为用了 `run_isolated`
（每个用例建独立临时目录）而不受影响——这也说明**测试必须显式隔离工作目录**，
依赖"进程默认 cwd"的用例天然脆弱。

**规避**：把 `/nctool.toml` 加进 `.gitignore`，手动测试改用
`nctool templates list --template-dir ./templates` 这类显式传参，
不在仓库里落配置文件。

#### 缺口六：`{% set x = x | default(v) %}` 被误判为必选（已修复）

源项目大量使用「自赋值兜底」惯用法：

```jinja
{% set R1 = R1 | default(4000) %}   {# 注释里、或在分支内 #}
...
G97 S1={{ R1 }} M1=3
```

语义是「外部给了就用，没给用 4000」。但移植 `UZ_DJ_X` 时实测发现渲染报：

```
错误 [R1] 必选参数缺失（模板引用且无默认值兜底，参数集未提供）（第 2 行第 12 列引用）
```

**根因在变量提取器**（`src/extract.rs`）：`record()` 无条件把「非兜底上下文」
的引用写进 `required_refs`。`{% set R1 = R1|default(4000) %}` 的 RHS 是兜底引用
（判为可选），但紧随其后的 `{{ R1 }}` 读的是**上一行 set 出来的模板局部量**，
该引用被当成外部引用记账，把 `R1` 整体翻回必选。

**修复**：`record()` 增加 `!self.is_local(name)` 条件 —— 模板局部变量的引用
不参与必选判定。注意 `{% set total = total + x %}` 里 RHS 的 `total` 仍记必选，
因为 `Stmt::Set` 的 RHS 先于目标声明求值（此刻 `total` 还不是局部量），
这一分支语义已由既有测试覆盖。

补 2 个回归测试：`extract_undeclared_set_self_default_stays_optional`、
`extract_undeclared_set_self_assign_without_default_is_required`（对照）。

#### 缺口七：头部注释不认空白控制 `-#}`（已修复）

模板为压掉头部注释产生的空行，惯用 `{# NAME: xxx -#}`。
旧 `extract_marker` 找 `#}` 之前**不剥 `-`**，于是显示名变成 `"xxx -"`，
在 `templates list` 里肉眼可见（描述末尾多一个 `-`）。

**修复**：取值前先 `trim_end_matches('-')`。补 2 个回归测试
（带 `-#}` 与不带 `-#}` 各一）。


### 4.3 建议的目标配置结构

整合后建议采用如下布局（本项目为 workspace 根）：

```
nctool-tpl/
├── templates/                      # 模板根（可递归）
│   ├── templates.yaml              # ★ 新增：模板清单
│   ├── variables.yaml              # ★ 新增：变量库（YAML 优于 JSON，可写注释）
│   ├── general/                    # 通用子程序
│   │   ├── program_header.j2
│   │   └── ...
│   ├── milling/
│   │   ├── slot_milling.j2
│   │   └── keyway/                 # 键槽族
│   ├── turning/
│   │   ├── undercut.j2
│   │   ├── sync_turning_left.j2
│   │   └── sync_turning_right.j2
│   ├── grooving/
│   │   └── circlip_groove.j2
│   └── machines/                   # ★ 机床方案包
│       └── index_g420/
│           ├── pack.yaml           # 包内清单
│           └── *.j2
```

**为什么用 YAML 而非 JSON 作变量库**：源项目 JSON 无法写注释，
而变量库恰恰最需要注释（单位、取值理由、工艺约束）。本项目已依赖 `serde`，
增加 `serde_yaml` 成本很低。

---

## 5. 可复用资产清单（逐项判定）

### 5.1 直接移植（Green）

| # | 资产 | 源位置 | 目标位置 | 工作量 | 说明 |
| --- | --- | --- | --- | --- | --- |
| G1 | 22 个 `.j2` 模板 | `templates/**/*.j2` | `templates/`（分类归档） | 小 | 内容资产，仅需替换过滤器名 |
| G2 | `templates.yaml` 清单约定 | `configs/templates.yaml` | `templates/templates.yaml` | 小 | schema 可直接沿用 |
| G3 | `{# NAME: #}` / `{# DESCRIPTION: #}` 约定 | 各模板头部 | 同名约定 | 极小 | 纯约定 |
| G4 | ES/FS 刀具标准数据库 | `core/undercut_logic.py:15-53` | `core/src/gear_db.rs` 或 `data/groove_standards.yaml` | 中 | 34 条 DIN509/W2196 标准记录 |
| G5 | 变量库 schema | `configs/variable_repo.json` | `variables.yaml` | 中 | 62 个变量的字段结构 |
| G6 | 数值格式化规则 | `format_float_clean` / `format_float_2_decimal` | 已有 `nc_strip` / `nc_fixed` | — | **已具备，无需移植** |

### 5.2 思路移植（Yellow）

| # | 资产 | 源实现 | Rust 对应实现 | 说明 |
| --- | --- | --- | --- | --- |
| Y1 | 变量 Repository | `VariableRepository`（370 行） | 扩展 `core/src/model.rs::ParamSpec` | 补 `Choice` 类型 + `options` + `associations` + `read_only` |
| Y2 | 惰性加载 | `lazy_load` 标志 | `OnceCell` / `LazyLock` | 减少启动耗时 |
| Y3 | 模板哈希缓存 | MD5 + `TemplateCache` | 已有（minijinja 编译缓存） | 已具备，可补文件 mtime 失效 |
| Y4 | 模板局部搜索路径 | `_local_search_path` 上下文管理器 | `Renderer::set_path_loader` | 已有，需扩展到**多目录** |
| Y5 | 递归变量提取 | `_scan_ast` + `meta.find_referenced_templates` | `nctool_tpl::extract_undeclared` | 本项目已实现**可选/必选区分**，**优于源项目** |
| Y6 | 异常层次 | `NCToolError` 家族 | 现有 `TplError` / `ValidationIssue` | 补结构化 `IssueKind`（已具备） |
| Y7 | 配置加载器 | `ConfigLoader`（泛型） | `cli/src/config.rs` | 已有层叠机制，更强 |
| Y8 | 错误码字典 | `error_messages.py` | `ValidationIssue::kind` | 已具备结构化分类 |

### 5.3 需重写（Red）

| # | 资产 | 原因 |
| --- | --- | --- |
| R1 | 全部 Python 实现 | 语言不同 |
| R2 | CustomTkinter UI（3300 行） | 本项目用 Web UI（`ui/index.html`） |
| R3 | `sys.frozen` / Nuitka 路径逻辑 | 打包方式不同（Rust 单二进制） |
| R4 | 模板内联计算（`sync_turning`） | 违反本项目「计算在 Rust 侧」的设计原则 |
| R5 | `error_handler.py` 的 Tk 对话框 | UI 层不同 |

### 5.4 优先级排序

按「价值 / 成本」排序，建议实施顺序：

```
P0（地基）  → G2 清单约定 + G3 元数据约定 + 内置模板外部化
P1（内容）  → G1 模板移植 + G4 刀具库 + G5 变量库
P2（能力）  → Y1 Choice 类型 + associations 联动 + read_only
P3（重构）  → R4 计算上移（sync_turning 重构）
P4（治理）  → 机床方案包（INDEX G420 归档）
```

### 5.5 机床模板移植实录（阶段 2，已完成）

源项目 `templates/INDEX G420/` 共 **19 个模板、约 2800 行**，
已全部移植到 `templates/machines/index_g420/`（文件名统一小写化）：

| 类别 | 个数 | 模板 |
| --- | --- | --- |
| 主程序（`.MPF`） | 6 | `1_0/2_0/3_0` × `A/B` 方案 |
| R 参数初始化（`.SPF`） | 1 | `dg_cal_ir9` |
| 键槽倒角（`.SPF`） | 1 | `uz_dj_x` |
| 键槽精铣（`.SPF`） | 4 | `uz_fkm_temp1~4` |
| 键槽粗铣（`.SPF`） | 7 | `uz_rkm_temp1~7` |

**移植手法**（可复现，脚本已一次性使用后删除）：

1. `"%.3f" | format(EXPR)` → `EXPR | nc_fixed(3)`；
   `%0.3f` 与 `%.3f` 输出完全一致，故统一映射到 3 位小数。
2. **`tan(x)` → `(x | tan_d)`**（关键）：源项目把 `sin/cos/tan` 作为
   **角度制**全局函数注入（`math.sin(math.radians(x))`），而本项目
   minijinja 侧没有同名全局函数，裸 `tan` 是弧度制过滤器。
   不改写会渲染失败；若误用弧度制则**静默算出错误坐标**。
   全量扫描确认只有 `tan` 被调用（`undercut.j2` 注释里提到的
   `tan_ES_ang` 等是说明文字，不在实际表达式中）。
3. 头部统一重写为 `{# NAME / DESCRIPTION / MACHINE / OUTPUT / PARAMS #}`
   五段式，`PARAMS` 由 `nctool inspect` 自动生成后回填，
   并用源 `variable_repo.json` 的 `description` 补中文说明。
4. `uz_dj_x.j2` 的 `R1~R6` 在源项目里只写在一段「不在最终输出中显示」
   的注释里、实为带默认值的参数，移植时改为文件头部的
   `{% set R1 = R1 | default(4000) %}` 显式兜底
   （这正是缺口六暴露出来的地方）。
5. 空白控制逐行对齐，保证产出与源模板**逐字节一致**。

**验证**（唯一硬证据，可复现）：用源项目的 Jinja2 环境
（含角度制全局函数 + `fmt_val`/`fmt_coord` 过滤器）渲染原模板作黄金样本，
与本项目渲染结果逐行比对：

```
18/18 逐行一致（归一化：行尾空白 + 源首行 `{# NAME #}` 残留空行）
```

未纳入比对的 `dg_cal_ir9.j2` 在更早的阶段已单独完成逐字节验证。

---

## 6. 整合方案

### 6.1 目标架构

```
┌─────────────────────────────────────────────────────────────┐
│  nctool-cli   (表现层)                                       │
│    templates / inspect / validate / render / machine / ui    │
│    ▲ 新增：templates discover --manifest                     │
└──────────────────────────┬──────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│  nctool-core  (编排层)  ← 本次整合主战场                      │
│                                                              │
│  ┌────────────────────┐   ┌──────────────────────────────┐  │
│  │ registry.rs        │   │ ★ manifest.rs (新增)         │  │
│  │  TemplateRegistry  │◄──│  templates.yaml 解析          │  │
│  │  （内存/文件/内置）│   │  可见性 / 输出名 / 后缀        │  │
│  └────────────────────┘   └──────────────────────────────┘  │
│                                                              │
│  ┌────────────────────┐   ┌──────────────────────────────┐  │
│  │ model.rs           │   │ ★ varlib.rs (新增)           │  │
│  │  ParamValue        │◄──│  variables.yaml 解析          │  │
│  │  ParamKind         │   │  + Choice / associations      │  │
│  │  ★ ParamKind::Choice   │  + read_only                 │  │
│  └────────────────────┘   └──────────────────────────────┘  │
│                                                              │
│  ┌────────────────────┐   ┌──────────────────────────────┐  │
│  │ ★ derive/ (新增)   │   │ pipeline.rs                  │  │
│  │  计算上移层         │──►│  校验 → 上下文 → 渲染 → 后处理│  │
│  │  sync_turning      │   └──────────────────────────────┘  │
│  │  undercut          │                                     │
│  │  circlip           │   validate.rs / machine.rs          │
│  └────────────────────┘                                     │
└──────────────────────────┬──────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│  nctool-tpl  (引擎层)  — 不直接依赖 minijinja（Value 再导出） │
│    parse / extract_variables / extract_undeclared / Renderer │
│    nc_fixed / nc_strip / nc_pad / 数学过滤器                  │
└─────────────────────────────────────────────────────────────┘
```

### 6.2 关键约定（建议写入 `docs/TEMPLATE_WRITING_GUIDE.md`）

**R1 — 模板名 = 相对路径**（含扩展名）
`templates/turning/undercut.j2` 的模板名为 `turning/undercut.j2`。
相对名唯一，且避免与内置模板冲突（现有约定已如此，需扩展到递归目录）。

**R2 — 模板头部必须声明元数据**
```jinja
{# NAME: 越程槽加工 #}
{# DESCRIPTION: 生成 ES/FS 型越程槽 G 代码，支持左右侧镜像 #}
{# PARAMS:
     groove_type  string  required  工艺类型: ES | FS
     side         string  required  侧面: Right | Left
     X_START      number  required  起始 X 坐标 (mm)
     #}
```
第三块 `{# PARAMS: #}` 是**相对源项目的增强**：源项目只有自由文本参数表，
这里改为机器可解析的三元组（名 / 类型 / 是否必选），可与 `nctool inspect` 交叉校验，
形成「**文档即测试**」——头部声明的参数若与实际引用不一致，`nctool lint` 报错。

**R3 — 模板不做算术，只做变量替换与格式化**
禁止在模板中出现 `**`、`tan(`、`sqrt(` 等计算（源项目 `UZ_DJ_X.j2` 违反此条）。
计算一律上移到 `core/src/derive/`。允许的例外：简单索引运算（`i + 1`）与 `default()` 兜底。

**R4 — 每个模板声明 `output_extension`**
`.NC`（通用）/ `.MPF`（主程序）/ `.SPF`（子程序）——沿用西门子/INDEX 约定。

**R5 — 机床专用模板必须归入方案包**
不得散落在 `templates/` 根目录，必须放在 `templates/machines/<machine_id>/`，
并由 `pack.yaml` 声明可见性与依赖的机床 id。

### 6.3 分阶段落地路线

#### 阶段 0 — 基建（无破坏性）

1. 新建 `docs/TEMPLATE_INTEGRATION_PLAN.md`（本文件）。
2. 在 `docs/TEMPLATE_WRITING_GUIDE.md` 中追加 §6.2 的 R1–R5 约定。
3. 新建 `templates/README.md` 说明目录分类规范。

**验收**：文档就位，无代码改动。

#### 阶段 1 — 模板清单与外部化（P0）

1. 新增 `core/src/manifest.rs`：
   - `TemplateManifest` / `TemplateMeta` 结构（`serde` 反序列化 `templates.yaml`）
   - `resolve(name) -> TemplateMeta`，实现「清单 > 头部注释 > 文件名」三级回退
2. 修改 `cli/src/context.rs::build_registry()`：
   - `read_dir` → 递归遍历（`walkdir` 或手写栈式遍历）
   - 接入清单，用 `visible` 过滤列表输出
   - 目录模板的分类从清单读取，而非硬编码 `TemplateCategory::General`
3. 给 `TemplateEntry` 补字段：`output_filename` / `output_extension` / `visible`。
4. `nctool templates list` 增加 `--all`（含隐藏）/ `--json`（含元数据）。

**验收**：
```bash
nctool templates list                    # 仅可见模板
nctool templates list --all --json       # 含隐藏 + 元数据
nctool render turning/undercut.j2 -p ... # 递归目录模板可渲染
```

**✅ 阶段 1 已实施完成**，实际落地与计划的差异（均为实测后的必要调整）：

| 项 | 计划 | 实际 | 原因 |
| --- | --- | --- | --- |
| 遍历方式 | `walkdir` 或手写栈式 | **手写递归**（`collect_templates`） | 避免新增依赖；顺带内建符号链接逃逸防护 |
| JSON 开关 | `--json` | 全局 `--format json` | 项目既有约定，无需新增开关 |
| 清单格式 | YAML（`serde_yaml`） | 同计划 | 需要写注释（如 `status: unreviewed # 迁移自…`） |
| 分类读取 | "从清单读取，而非硬编码 General" | **清单 > 目录名推断 > General** | 清单不应成为必填——目录结构已能表达分类时无需重复声明 |
| 额外发现 | — | **`ParamValue` 缺列表类型** | 循环驱动模板无法移植，见 §4.4 缺口一 |
| 额外发现 | — | **`TemplateCategory` 缺 `Grooving`** | `grooving/` 目录会静默落到「通用」分类，见下 |

> **`Grooving` 分类的补充**：原计划的分类枚举只有
> 通用/铣削/车削/钻孔/机床五项，而 §4.3 的目标布局里有 `grooving/` 目录。
> 若不补，该目录下的模板会静默归类为「通用」——不是报错，只是列表里
> 分类显示错误，属于难以察觉的低级错误。已补 `TemplateCategory::Grooving`（「切槽」），
> 并在 `classify_by_path` 与 CLI 的 `CategoryArg`（含「切槽」别名）同步。

#### 阶段 2 — 模板内容移植（P1）

1. 按 §4.3 创建分类目录，移植 22 个模板。
2. 过滤器映射（**关键**）：

   | 源（Jinja2） | 目标（minijinja） | 状态 |
   | --- | --- | --- |
   | `fmt_coord` → `f"{v:+.3f}"` | **`nc_signed(3)`**（已新增） | ✅ 已实现 |
   | `fmt_val` → `f"{v:.3f}"` | `nc_fixed(3)` | ✅ 直接可用 |
   | `"%.3f"\|format(v)` | `v \| nc_fixed(3)` | ✅ 改为过滤器形式 |
   | `sin/cos/tan(x)`（**度**） | **`sin_d` / `cos_d` / `tan_d`**（已新增） | ✅ 已实现 |
   | `pi` | `pi` | ✅ 已具备 |

   > ⚠️ **度制/弧度制陷阱**：源项目 `env.globals` 里的 `sin(x)` 定义为
   > `math.sin(math.radians(x))`——输入是**角度**。本项目裸数学过滤器为弧度制，
   > 另有 `_d` 后缀的度制版本。**同名不同义**，移植时若照抄 `| sin` 会静默产生
   > 错误坐标（`sin(30)` = -0.988 而应为 0.5）。**一律改用 `_d` 版本。**
   > 详见 §4.4 与 `docs/TEMPLATE_WRITING_GUIDE.md` §4。

   > **`nc_signed` 的边界（实测补充）**：该过滤器只为**增量坐标/旋转量**准备
   > ——正数与零也输出 `+`。源项目唯一的使用点是 `circlip_groove_template.j2`，
   > 且只出现在一行**注释**里（`; --- 卡簧槽 Z=... ---`），改用 `nc_signed`
   > 后注释显示为 `Z=+261.000`，与源项目输出一致。**不要**用它格式化直径、进给等
   > 本无符号语义的值。另：`-0.0` 会归一到 `+0.000`（控制器对负零处理不一致）。

3. 移植 `ES_GROOVE_DB` / `FS_GROOVE_DB` 到 `data/groove_standards.yaml`。
4. 移植变量库到 `variables.yaml`（62 个变量，JSON→YAML）。

**验收**：`nctool inspect` 对每个移植模板输出的必选/可选参数，与源项目 Python 提取结果一致。

#### 阶段 3 — 参数库能力补齐（P2）

1. `ParamKind` 增加 `Choice` 变体；`ParamValue` 保持 4 值类型不变
   （`Choice` 是**约束**而非**存储类型**——值是 `String`/`Integer`/`Number`，
   `Choice` 通过 `ParamSpec.options` 约束取值）。
2. `ParamSpec` 增加字段：`options: Vec<String>` / `read_only: bool` /
   `bound_area: Option<String>` / `associations: BTreeMap<String, BTreeMap<String, ParamValue>>`。
3. `validate.rs` 新增 `IssueKind::NotInOptions`。
4. `derive/` 实现联动求值：主变量确定后，按 `associations` 反查并写入被动变量。
   **注意**：联动求值须在**校验前**执行，否则只读变量会被误报为「必选参数缺失」。

**验收**：`nctool validate` 能拒绝不在 `options` 中的取值；联动模板渲染时被动变量被正确填充。

#### 阶段 4 — 计算上移重构（P3）

1. 新建 `core/src/derive/`：
   - `sync_turning.rs`：`plan_cuts(...) -> Vec<CutPoint>`
   - `undercut.rs`：ES/FS 几何量计算（源 `undercut_logic.py` 的 `derived_params`）
2. 重写 `sync_turning_left/right.j2` 为 §3.4 的数组遍历形式。
3. 为每个 `derive` 函数补单元测试，覆盖左右侧镜像与边界刀次数。

**验收**：重构前后对同一组输入，G-code 输出**逐字节一致**（用源 Python 版本产出的结果做黄金样本）。

#### 阶段 5 — 机床方案包（P4）

1. `INDEX G420/*.j2` 移入 `templates/machines/index_g420/`。
2. 新增 `pack.yaml`：
   ```yaml
   machine_id: index_g420
   label: INDEX G420
   description: INDEX G420 车铣复合机床专用程序包
   templates:
     - path: 1_0.MPF_A.j2
       name: "(A)1.MPF 同步车右侧"
       visible: true
       output_filename: "1_0"
       output_extension: ".MPF"
   ```
3. `MachineConfig` 增加 `template_pack: Option<String>` 字段，
   `nctool templates list --machine index_g420` 时暴露对应包内模板。

**验收**：未指定机床时，方案包模板不出现在列表中；指定后可见且可渲染。

---

## 7. 风险与对策

| # | 风险 | 影响 | 对策 |
| --- | --- | --- | --- |
| 1 | **度制/弧度制陷阱** | 所有含三角函数的模板静默产出错误坐标 → **撞刀风险** | 阶段 2 强制逐模板核对；新增 `sin_d/cos_d/tan_d`；移植后与原版对拍 |
| 2 | 模板内联计算重构引入偏差 | G-code 数值漂移 | 用源 Python 版本产出**黄金样本**，重构后逐字节比对 |
| 3 | 源模板未做工艺评审 | 移植的模板本身可能有工艺错误 | 保留源项目的风险声明；移植模板标记 `status: unreviewed`；**投产前必须空运行** |
| 4 | 机床专有 G 代码不通用 | 方案包在非目标机床上不可用 | 方案包强绑定 `machine_id`，未指定机床时不可见；渲染时校验机床匹配 |
| 5 | `read_dir` → 递归遍历的路径安全 | 符号链接逃逸、路径遍历 | 沿用现有 `canonicalize` + `starts_with(root)` 校验（`context.rs:81-84`） |
| 6 | 变量库 JSON→YAML 迁移丢字段 | 校验规则失效 | 写一次性迁移脚本 + 往返测试（YAML→结构体→YAML 应等价） |
| 7 | 两个项目并行演进导致分叉 | 资产重复维护 | 明确「源项目为只读资产源」；标记归档 commit SHA |

### 7.1 关于风险 1 的补充说明

这是**最高优先级的安全问题**。举例说明：

```jinja
{# 源项目：sin 接受角度 #}
{% set XIANGAO = (U_D/2) - ((U_D/2)**2 - (U_B/2)**2)**0.5 %}
```

```jinja
{# 若含三角函数的写法（源语义） #}
{{ sin(30) }}        {# 源项目 = 0.5（30度） #}
{{ sin(30) }}        {# 若直接移植 = -0.988（30弧度）← 完全错误 #}
```

两者差异是**量级级别的**，且不会报错。必须在移植阶段逐个模板核对，
并在 `nctool lint` 中增加「模板内出现三角函数」的告警（提示确认度制）。

---

## 8. 交付物清单

| 交付物 | 路径 | 类型 |
| --- | --- | --- |
| **本整合方案** | `docs/TEMPLATE_INTEGRATION_PLAN.md` | 新增文档 |
| 模板编写约定增补 | `docs/TEMPLATE_WRITING_GUIDE.md`（追加 R1–R5） | 修改 |
| 模板清单 | `templates/templates.yaml` | 新增配置 |
| 变量库 | `templates/variables.yaml` | 新增配置 |
| 刀具标准库 | `data/groove_standards.yaml` | 新增数据 |
| 清单解析 | `core/src/manifest.rs` | 新增模块 |
| 变量库解析 | `core/src/varlib.rs` | 新增模块 |
| 计算上移层 | `core/src/derive/{mod,sync_turning,undercut,circlip}.rs` | 新增模块 |
| 递归模板发现 | `cli/src/context.rs` | 修改 |
| `Choice` 参数类型 | `core/src/model.rs` / `core/src/validate.rs` | 修改 |
| 移植模板 | `templates/{general,milling,turning,grooving,machines}/**/*.j2` | 新增内容 |

---

## 9. 附：源仓库资产统计

| 维度 | 数值 |
| --- | --- |
| 文件总数（不含 `.git`） | 78 |
| Python 文件 | 35（约 6700 行） |
| `.j2` 模板 | 22 |
| Markdown 文档 | 6 |
| JSON 配置 | 4 |
| YAML 配置 | 1 |
| 模板总行数 | 约 2700 行 |
| 变量库变量数 | 62 |
| 刀具标准记录 | 34（ES 19 + FS 15） |
| UI 代码占比 | 约 49%（3300 / 6700 行） |

**关键观察**：UI 代码占了整个源仓库的一半，而 UI 恰恰是**唯一不可移植**的部分。
本次整合的净收益集中在：
- 22 个模板（约 2700 行内容资产）
- 5 套配置约定
- 34 条刀具标准数据
- 62 个变量的定义与校验规则

---

## 10. 参考

- 源仓库：`https://github.com/lzg9698-code/NCTool_V3.git`（分析基线：`main` @ `0c2c9a7`）
- 源项目架构文档：`docs/ARCHITECTURE.md`
- 本项目架构文档：`docs/ARCHITECTURE.md`
- 本项目模板指南：`docs/TEMPLATE_WRITING_GUIDE.md`
- 本项目机床配置指南：`docs/MACHINE_CONFIG_GUIDE.md`
