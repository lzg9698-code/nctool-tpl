# nctool-tpl

NCtool 模板解析核心：基于 [minijinja](https://github.com/mitsuhiko/minijinja) 的 Jinja2 模板解析 + 变量提取 + 渲染，面向数控加工 G-code 模板场景。

- 近乎零依赖（仅 minijinja 一个 crate），Jinja2 作者本人维护的高性能引擎
- 对标 Python `jinja2.meta`：能从模板中提取「引用的全部变量」与「需要外部上下文提供的未声明变量」
- 内置一组数学过滤器（`f64` 标准库实现），供 G-code 计算使用

> 本仓库是 workspace，含 `nctool-tpl`（本包）/ `nctool-core` / `nctool-cli` 三个 crate。
> 想了解整体架构、模块职责与数据流，请看 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。
>
> ⚠️ **定位与风险声明（必读）**：本项目定位为**模板开发工具**，内置模板与机床预设
> 输出尚未经真实工艺评审与机床空运行验证。G/M 代码错误可能导致撞刀、刀具或设备
> 损坏——**投产前必须由熟悉目标机床的工艺人员逐行核对，并在机床上空运行确认**
> （详见 [docs/PROCESS_CHECKLIST.md](docs/PROCESS_CHECKLIST.md) 的核对清单与
> 【阶段 A】发现项 F1–F5）。机床预设默认值仅供开发测试。

## 目录

- [安装](#安装) · [功能](#功能) · [NC 数值格式化过滤器](#nc-数值格式化过滤器) · [严格 / 宽松模式](#严格--宽松模式)
- [性能基线](#性能基线) · [错误类型](#错误类型) · [多模板渲染](#多模板渲染) · [快速开始](#快速开始)
- [使用示例](#使用示例) · [命令行工具 nctool](#命令行工具-nctoolnctool-cli) · [退出码](#退出码)
- [可选 / 必选判定规则](#可选--必选判定规则) · [数学过滤器](#数学过滤器) · [示例与测试](#示例与测试)
- [定位精度与判定边界](#定位精度与判定边界) · [贡献指南](#贡献指南) · [相关文档](#相关文档)

## 安装

### 环境要求

| 项目 | 要求 |
| --- | --- |
| Rust 工具链 | **1.82 及以上**（`rust-version = "1.82"`；CI 使用 stable） |
| 操作系统 | Linux / macOS / Windows（CI 三平台矩阵均需绿灯） |
| 运行时依赖 | 无。CLI 是单一二进制，release 构建开启 LTO + strip |

### 方式一：下载预编译二进制（无需 Rust 环境）

从 [GitHub Releases](https://github.com/lzg9698-code/nctool-tpl/releases) 下载对应平台产物，
改名后放入 `PATH` 中任意目录即可：

| 平台 | 产物文件名 | 架构 |
| --- | --- | --- |
| Linux | `nctool-x86_64-unknown-linux-gnu` | x86_64 |
| Windows | `nctool-x86_64-pc-windows-msvc.exe` | x86_64 |
| macOS | `nctool-aarch64-apple-darwin` | Apple Silicon（Intel 版待需求） |

```bash
# Linux / macOS
chmod +x nctool-x86_64-unknown-linux-gnu
mv nctool-x86_64-unknown-linux-gnu /usr/local/bin/nctool

# Windows（PowerShell，放到用户级 bin 目录并加入 PATH）
# Move-Item nctool-x86_64-pc-windows-msvc.exe "$Env:LOCALAPPDATA\nctool\nctool.exe"
```

### 方式二：从源码安装 CLI

```bash
git clone https://github.com/lzg9698-code/nctool-tpl.git
cd nctool-tpl
cargo install --path cli --locked      # 二进制安装到 ~/.cargo/bin/nctool
nctool --version                       # nctool 0.2.2
```

只想临时试用、不安装到全局，可直接从工作区运行：

```bash
cargo run -p nctool-cli -- templates list
```

### 方式三：作为 Rust 库引入

只需「模板解析 + 变量提取 + 渲染」：

```toml
[dependencies]
nctool-tpl = "0.3"
```

还需要「参数校验 + 模板注册表 + 机床配置 + G-code 生成管线」：

```toml
[dependencies]
nctool-core = "0.2"      # 会一并带入 nctool-tpl
```

等价命令：`cargo add nctool-tpl@0.3` / `cargo add nctool-core@0.2`。
若对应版本尚未发布到 crates.io，可改用 git 依赖：

```toml
[dependencies]
nctool-core = { git = "https://github.com/lzg9698-code/nctool-tpl" }
```

### 验证安装

```bash
nctool templates list        # 应列出 7 个内置模板
nctool machine show generic  # 应打印内建机床预设
```

## 功能

| API | 说明 |
| --- | --- |
| `parse(source, name)` | 语法检查并生成 AST（带行列定位） |
| `extract_variables(&ast)` | 提取模板中**引用过**的全部变量名（含模板内部声明的） |
| `extract_undeclared(&ast)` | 提取引用但**未在模板内声明**的变量 —— 即渲染时必须由外部提供的参数；并区分**可选 / 必选** |
| `Renderer` | 用上下文渲染出最终文本（G-code），内置数学过滤器集；支持多模板（`include`/`extends`/`import`） |

## NC 数值格式化过滤器

G-code 对数值格式敏感，`Renderer` 内置三个专用过滤器：

| 过滤器 | 用法 | 输入 | 输出 | 用途 |
| --- | --- | --- | --- | --- |
| `nc_fixed(N)` | `{{ x \| nc_fixed(3) }}` | `21.0` | `21.000` | 固定小数位的坐标值 |
| `nc_strip` | `{{ x \| nc_strip }}` | `21.0` | `21` | 去尾零，避免 `X21.0` |
| `nc_pad(N)` | `{{ n \| nc_pad(4) }}` | `1` | `0001` | 程序号/行号前导零 |

```jinja
O{{ prog | nc_pad(4) }}
N{{ line | nc_pad(4) }} G1 X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }} F{{ feed | nc_strip }}
```

渲染结果（prog=1, line=10, x=21.0, y=15.5, feed=0.150）：
```
O0001
N0010 G1 X21.000 Y15.500 F0.15
```

所有 NC 过滤器对非有限数（NaN/Inf）报错，防止非法数值写入 G-code。该防线覆盖本库注册的过滤器；裸 `{{ x }}` 输出或 minijinja 内建操作（如 `round`、算术）产生的 NaN/Inf 不经此校验，请先经上层参数校验（`nctool-core`）保证参数值有限。

## 严格 / 宽松模式

`Renderer` 默认**严格模式**：引用未定义变量直接渲染失败，避免静默输出不完整 G-code。需要宽松渲染时用 `with_lenient()` 切换：

```rust
// 严格模式（默认）：未定义变量报错
let r = Renderer::new();
r.render("X{{ x }}", "t.j2", &minijinja::context!{})?;  // Err(UndefinedVariable)

// 宽松模式：未定义变量渲染为空字符串
let r = Renderer::new().with_lenient();
r.render("X{{ x }}", "t.j2", &minijinja::context!{})?;  // Ok("X")
```

典型流程：`extract_undeclared` 先校验必选参数是否齐全，校验通过后再用严格模式渲染（保证输出完整）；宽松模式适合「参数可缺省、缺失即留空」的柔性模板。`is_lenient()` 可查询当前模式。

## 性能基线

基于 criterion benchmark（`cargo bench`），中等复杂度 G-code 模板（~260 字节，含 set/default、数学过滤器、for 循环、if 条件）：

| 操作 | 耗时（中位数） | 吞吐 |
| --- | --- | --- |
| `parse` | 2.61 µs | 98 MiB/s |
| `extract_undeclared` | 1.97 µs | — |
| `render` | 5.32 µs | 48 MiB/s |

生成管线侧（`nctool-core`，`cargo bench -p nctool-core --bench pipeline`）：

| 操作 | 耗时（中位数） | 吞吐 |
| --- | --- | --- |
| 端到端 `generate`（drill_cycle） | 9.54 µs | — |
| 后处理 3000 行（仅清空行） | 386.89 µs | 142 MiB/s |
| 后处理 3000 行 + 行号 | 438.10 µs | 126 MiB/s |
| 后处理 3000 行 + ASCII 清洗 | 516.04 µs | — |

行号前缀每行约 17 ns，规模增大时可据此外推。

**万行级实测**（`cargo test --release -p nctool-core --test large_program -- --ignored`）：
10000 行带行号 **1.77 ms / 203 KB**。行号位宽取自机床配置且已夹紧到 32 位上限——
即便把 `line_number_digits` 配成 `1000000000`，输出也只到 **231 KB / 1.81 ms**，
不会出现内存放大（位宽若缺失上界，Rust 的分配失败是进程 abort，不可捕获）。

测试环境：release 构建（LTO + strip + codegen-units=1）。重复渲染相同模板时，建议用 `add_template` + `render_template`（minijinja 会缓存编译结果），避免每次重新编译。

## 错误类型

`TplError` 已细分，上层可精准处理（`#[non_exhaustive]`，match 请保留通配分支）：

| 变体 | 触发场景 | 关键字段 |
| --- | --- | --- |
| `Parse` | 模板语法错误 | `line`, `col`（真实行列定位） |
| `TemplateNotFound` | `include`/`extends`/`get_template` 引用了不存在的模板 | `template` |
| `UndefinedVariable` | Strict 模式下引用了未定义变量 | —（minijinja 不携带变量名，可结合模板源码定位） |
| `UnknownFilter` | 使用了未注册的过滤器 | `filter` |
| `UnknownTest` | 使用了未注册的测试 | `test` |
| `Render` | 其他渲染错误（无效操作、参数错误等兜底） | — |

## 多模板渲染

`Renderer` 支持模板间引用，两种注册方式：

```rust
use nctool_tpl::Renderer;

let mut r = Renderer::new();

// 方式一：内存注册（owned 字符串，无生命周期约束）
r.add_template("header.j2", "O{{ prog }} ({{ name }})").unwrap();
r.add_template("main.j2", "{% include \"header.j2\" %}\nG1 X{{ diameter / 2 }}").unwrap();

// 方式二：从文件系统目录动态加载（按需加载并缓存）
// r.set_path_loader("templates/");

let ctx = minijinja::context! { prog => 1000, name => "DEMO", diameter => 42.0 };
let out = r.render_template("main.j2", &ctx).unwrap();
// out = "O1000 (DEMO)\nG1 X21.0"
```

`{% include %}` / `{% extends %}` / `{% import %}` 均能正确解析到已注册或目录中的模板。

每个返回的 `Variable` 都带有 `optional: bool` 字段：`true` 表示该变量的**全部引用**都处于「兜底上下文」（作为 `default`/`d` 过滤器或 `is defined`/`is undefined` 测试的**直接裸变量操作数**）——对 `extract_undeclared` 而言即**可选参数**（缺失时模板仍可安全渲染），`false` 为**必选参数**。详见下方[可选 / 必选判定规则](#可选--必选判定规则)。

## 快速开始

```rust
use nctool_tpl::{parse, extract_undeclared, Renderer, Variable};

let source = r#"{% set feed = 0.15 %}G1 X{{ diameter / 2 }} F{{ feed }}"#;
let ast = parse(source, "demo.j2").unwrap();

// 未声明变量 = 需要外部上下文提供的参数
let undeclared: Vec<Variable> = extract_undeclared(&ast);
assert_eq!(undeclared.len(), 1);
assert_eq!(undeclared[0].name, "diameter");
assert!(!undeclared[0].optional); // 必选

// 可选参数：有 default 兜底 / defined 检查
let src = "G1 F{{ feed | default(0.15) }} {% if coolant is defined %}M8{% endif %}";
let vars: Vec<Variable> = extract_undeclared(&parse(src, "o.j2").unwrap());
assert!(vars.iter().all(|v| v.optional)); // feed / coolant 均为可选

// 渲染（Strict 模式：缺失变量直接报错，不静默输出不完整 G-code）
let renderer = Renderer::new();
let ctx = minijinja::context! { diameter => 42.0 };
let out = renderer.render(source, "demo.j2", &ctx).unwrap();
assert_eq!(out, "G1 X21.0 F0.15");
```

## 使用示例

以下示例的命令与输出均为**实测结果**（`nctool 0.2.2`），可逐条复制执行。
命令面全貌见 [命令行工具](#命令行工具-nctoolnctool-cli)，库 API 见[快速开始](#快速开始)。

### 例 1：内置模板端到端 —— 浏览 → 查参数 → 校验 → 生成

```bash
# 1) 有哪些模板
nctool templates list
```

```
模板列表（7 个）
drill_cycle              钻孔     钻孔循环：G81 标准钻孔
facing                   铣削     面铣：矩形区域往复行切（zigzag）
program_footer           通用     程序尾：主轴/冷却关闭 + 取消循环 + 程序结束 + 纸带结束符
program_header           通用     程序头：纸带起始符 + 程序号 + 注释头 + 单位/坐标系/取消态初始化
safe_move                通用     安全移动：抬刀到安全高度 + 定位
slot_milling             铣削     键槽铣：X 方向直槽一刀成型（下刀→切削→抬刀→返回）
tool_change              通用     换刀：主轴停止 + 换刀 + 刀长补偿 + 启动主轴与冷却
```

```bash
# 2) 这个模板需要哪些参数（带行列定位）
nctool inspect drill_cycle
```

```
模板: drill_cycle
必选参数（5）:
  x  行 1 列 24
  y  行 1 列 47
  r_plane  行 2 列 12
  depth  行 2 列 41
  feed  行 2 列 68
可选参数（0）:
```

```bash
# 3) 参数不全时，validate 报错并返回退出码 1（不产出任何 G-code）
nctool validate drill_cycle --param x=21
echo $?   # 1
```

```
模板: drill_cycle
错误 [y] 必选参数缺失（模板引用且无默认值兜底，参数集未提供）（第 1 行第 47 列引用）
错误 [depth] 必选参数缺失（模板引用且无默认值兜底，参数集未提供）（第 2 行第 41 列引用）
错误 [feed] 必选参数缺失（模板引用且无默认值兜底，参数集未提供）（第 2 行第 68 列引用）
```

```bash
# 4) 参数补齐后生成：加行号 + 头部注释 + 写文件
nctool render drill_cycle --param x=21 --param y=15 --param r_plane=2 \
    --param depth=-10 --param feed=100 --line-numbers --header --out demo.nc
```

`demo.nc` 内容：

```
( ================================== )
( nctool generated G-code )
( template: drill_cycle )
( ================================== )
N0010 G0 X21.000 Y15.000
N0020 G98 G81 R2.000 Z-10.000 F100.000
N0030 G80 (取消循环)
```

### 例 2：写自己的模板文件

`chamfer.j2`：

```jinja
( 倒角：{{ diameter }} 外圆，C{{ chamfer }} )
G0 X{{ (diameter / 2 - chamfer) | nc_fixed(3) }} Z{{ z_start | nc_fixed(3) }}
G1 X{{ (diameter / 2) | nc_fixed(3) }} Z{{ (z_start - chamfer) | nc_fixed(3) }} F{{ feed | nc_strip }}
```

```bash
nctool inspect chamfer.j2      # 直接传文件路径即可
nctool render chamfer.j2 --param diameter=40 --param chamfer=1 \
    --param z_start=0 --param feed=0.12
```

```
( 倒角：40.0 外圆，C1.0 )
G0 X19.000 Z0.000
G1 X20.000 Z-1.000 F0.12
```

写模板前建议先读 [《模板编写指南》](docs/TEMPLATE_WRITING_GUIDE.md)；`nc_fixed` / `nc_strip` / `nc_pad`
的行为见 [NC 数值格式化过滤器](#nc-数值格式化过滤器)。

### 例 3：参数文件 + 脚本/CI 集成

参数多时用 JSON 文件批量传入，显式 `--param` 可覆盖文件里的值：

```bash
cat > params.json <<'EOF'
{ "x": 21, "y": 15, "r_plane": 2, "depth": -10, "feed": 100 }
EOF

nctool render drill_cycle --params-file params.json --param feed=80
```

脚本里用 `--format json` + 退出码判分支（成功 `{"ok":true,"data":...}`，失败 `{"ok":false,"error":{...}}`）：

```bash
out=$(nctool render drill_cycle --params-file params.json --format json)
rc=$?
if [ $rc -ne 0 ]; then
  echo "生成失败（退出码 $rc）" >&2
  exit $rc
fi
echo "$out" | jq -r '.data.output' > program.nc
```

失败时的 JSON（节选）：

```json
{
  "data": {
    "errors": 3,
    "template": "drill_cycle",
    "issues": [
      { "level": "error", "param": "y",
        "message": "必选参数缺失（模板引用且无默认值兜底，参数集未提供）（第 1 行第 47 列引用）" }
    ]
  },
  "error": { "kind": "validation", "message": "错误 [y] 必选参数缺失…" },
  "ok": false
}
```

### 例 4：目录模板与配置

`templates new` 生成的骨架落在 `./templates/` 下，但**目录模板不会自动被加载**——
需要通过 `nctool.toml` 的 `template_dir` 或全局参数 `--template-dir` 指定目录：

```bash
nctool templates new chamfer            # 生成 templates/chamfer.j2 骨架
nctool config init                      # 生成 ./nctool.toml（示例中 template_dir 默认被注释）
# 取消注释生效：template_dir = "templates"

nctool templates list                   # 此时列表里多出 chamfer.j2（共 8 个）
nctool --template-dir templates inspect chamfer.j2   # 或临时用全局参数指定
```

> **注意**：目录模板的模板名是**完整文件名（含 `.j2` 后缀）**，如 `chamfer.j2`；
> 内置模板名不带后缀（如 `drill_cycle`）。这样两者不会重名冲突。
> 配置层级：项目 `./nctool.toml`（向上递归查找）覆盖全局 `~/.config/nctool/config.toml`。
> 详见 [《机床配置指南》](docs/MACHINE_CONFIG_GUIDE.md)。

### 例 5：Web UI / 库调用

```bash
# 本地 Web UI（仅绑定回环地址）：模板浏览 + 参数表单 + 防抖预览 + 校验定位 + 下载
nctool ui --host 127.0.0.1 --port 8787
```

库调用见[快速开始](#快速开始)（解析 → 变量提取 → 渲染三段式），
完整管线（校验 + 机床配置 + 后处理）见 [core/README.md](core/README.md)。

## 命令行工具 nctool（nctool-cli）

工作区新增 `cli/` crate，提供二进制 `nctool`，覆盖模板浏览、变量提取、参数校验与 G-code 生成全流程（基于 `nctool-core` 管线，golden 测试保证输出逐字节一致）：

```bash
# 浏览内置模板
nctool templates list
nctool templates show drill_cycle        # 查看源码与参数表
nctool templates new my_op               # 在当前 templates/ 下新建骨架

# 提取模板必选/可选参数（含行列定位）
nctool inspect drill_cycle

# 参数校验（缺失必选 → 退出码 1 + 结构化报告）
nctool validate drill_cycle --param x=21 --param y=15 --param depth=-10 --param feed=100

# 生成 G-code：行号 + 头部注释 + 写文件
nctool render drill_cycle --param x=21 --param y=15 --param depth=-10 --param feed=100 \
    --line-numbers --header --out demo.nc

# `generate` 与 `render` 同签名，是后处理全开时的规范入口（默认输出逐字节一致）
nctool generate drill_cycle --param x=21 --param y=15 --param depth=-10 --param feed=100

# 参数文件（JSON）批量输入；显式 --param 覆盖文件值
nctool render my_op.j2 --params-file params.json

# 机床配置查看 / 配置初始化 / shell 补全
nctool machine show wfl_m65
nctool config init
nctool completion bash          # 另支持 zsh / fish / elvish 及 Windows 系 shell

# 启动本地 Web UI（模板浏览 + 只读 API；仅绑定回环地址）
nctool ui --host 127.0.0.1 --port 8787

# 机器可读输出（--format json）：成功 {"ok":true,"data":...}，失败 {"ok":false,"error":{...}}
nctool render drill_cycle --param x=21 --param y=15 --param depth=-10 --param feed=100 --format json
```

运行方式：开发 `cargo run -p nctool-cli -- <命令>`；安装 `cargo install --path cli` 后直接使用 `nctool`。

### 退出码

`nctool` 用退出码传递失败类型，便于脚本判分支：

| 码 | 含义 | 典型触发 |
| --- | --- | --- |
| 0 | 成功 | — |
| 1 | 参数校验未通过 | 缺失必选参数、类型不匹配 |
| 2 | 参数/用法错误（与 clap 一致） | 未知子命令、`templates new ../x`（含路径分隔符）、`ui --host 0.0.0.0` |
| 3 | IO 失败 | `--params-file` 指向不存在的文件 |
| 4 | 配置错误 | `config init` 时 `nctool.toml` 已存在 |
| 5 | 模板/机床未找到 | `inspect nope`、`machine show nope` |
| 6 | 渲染/注册表失败 | `templates new` 重名（重名是业务冲突，非 IO） |
| 7 | 功能尚未实现 | `part generate`（规划于阶段 4） |

该矩阵是稳定的对外契约，由 `cli/tests/cli_e2e.rs` 的 44 个 E2E 用例逐条断言
（覆盖全部 10 个子命令 × 正常/异常路径）；变更退出码必须同步更新该测试与 CHANGELOG。

## 可选 / 必选判定规则

- **可选**：变量的**全部**引用都是 `x | default(默认值)`（别名 `d`）或 `x is defined` / `x is undefined` 的**直接裸变量操作数**。
- **必选**：变量在任意非兜底位置被引用（如 `{{ x }}`、`{{ x / 2 }}`、过滤器/函数参数等），或既有兜底引用又有非兜底引用。

**兜底不向下传播**——这是最容易踩的坑。minijinja 会**先求值操作数、再套用过滤器/测试**，
因此只有裸变量能被安全兜底；操作数是运算、属性或下标时，undefined 参与求值即直接报错，
`default` / `defined` 根本来不及生效：

| 模板 | 判定 | 原因 |
| --- | --- | --- |
| `{{ x \| default(1) }}` | `x` **可选** | 操作数即裸变量，undefined 被兜底 |
| `{{ x \| default(1) \| nc_fixed(3) }}` | `x` **可选** | 兜底后串接过滤器仍安全 |
| `{% if x is defined %}` | `x` **可选** | 同上 |
| `{{ (a+b) \| default(1) }}` | `a`、`b` **必选** | 先算 `a+b`，undefined 参与运算即报错 |
| `{{ a.b \| default(1) }}` | `a` **必选** | 先对 undefined 的 `a` 取属性，报错 |
| `{% if a.b is defined %}` | `a` **必选** | 同上 |

若把后三类误判为可选，上层校验会放行、严格模式渲染却失败，产出**不完整的 G-code**。
因此判定策略是**宁多勿漏**：有疑问即记为必选。
- 模板内部 `set`/`for`/`with`/宏参数等声明的局部变量不进未声明集合，不受此规则影响。

## 数学过滤器

全部基于 Rust 标准库 `f64`，零额外依赖：

`sin` `cos` `tan` `asin` `acos` `atan` `sqrt` `exp` `ln` `log10` `pow` `floor` `ceil`

**有限性校验**：所有数学过滤器对结果做 `is_finite()` 检查，一旦产生 `NaN`/`Inf`（如 `sqrt(-1)`、`ln(0)`），渲染立即失败并报错，避免非法坐标静默写入 G-code。同 NC 过滤器一样，该防线仅覆盖本库注册的过滤器；裸输出与内建操作不在保护范围。

## 示例与测试

```bash
# 运行可执行示例（解析 + 变量提取 + 渲染 templates/demo_gcode.j2）
cargo run --example demo

# 运行全部测试（单元 + 集成 + 文档）
cargo test

# 静态检查与格式
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## 定位精度与判定边界

- **解析错误列号**：`TplError::Parse.col` 取自 minijinja 错误携带的字节范围（需启用 `debug` feature）换算而来，指向**解析器停止处**的 token，是对错误位置的最佳近似（多数场景精确，个别场景如"未闭合块"只精确到行）。无法取得字节范围时回退为 `col = 1`。
- **可选 / 必选判定边界**：只把 `default`/`d` 过滤器与 `defined`/`undefined` 测试的**直接裸变量操作数**记为可选，兜底**不向下传播**到子树（`(a+b) | default(1)`、`a.b | default(1)`、`a.b is defined` 中的变量均记为必选）；`defined` 保护块**内部**的引用仍记为必选（保守策略，宁多勿漏）；`default(参数)` 的默认值表达式里的变量仍记为必选（它必须存在才能求值默认值）。

## 贡献指南

欢迎提 issue / PR。完整版见 [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md)，速览如下。

### 开始之前

- 先读 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)（三层 crate 与数据流）和
  [docs/ROADMAP.md](docs/ROADMAP.md)（阶段计划与 Backlog），避免方向冲突或重复劳动。
- 涉及模板 / 机床的内容，请务必先读开头的**定位与风险声明**与
  [docs/PROCESS_CHECKLIST.md](docs/PROCESS_CHECKLIST.md)：内置模板与机床预设**未经真实工艺评审**，
  工艺正确性问题**不属于工具 bug**，不按 issue 流程处理。

### 开发环境

```bash
git clone https://github.com/lzg9698-code/nctool-tpl.git
cd nctool-tpl
cargo build --workspace
cargo test --workspace              # 344 项（2026-09-11 实测全绿）
cargo install cargo-audit --locked  # 安全审计，CI 必查
```

### 提交前质量门（与 CI 完全一致）

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
cargo audit
```

> Windows 上 `cargo package` 在 rustc 1.98 有打包期 ICE 的已知问题，
> 需加 `CARGO_INCREMENTAL=0`：`CARGO_INCREMENTAL=0 cargo package --workspace --allow-dirty`。

### 改动要求

| 改了什么 | 必须同步更新 |
| --- | --- |
| 公共 API / 错误类型 / 行为 | `CHANGELOG.md` 的 `Added` / `Changed` 节 + 相关文档；破坏性变更要写迁移方式 |
| CLI 退出码或 `--format json` 字段 | `cli/tests/cli_e2e.rs`（44 个用例）+ CHANGELOG —— 这两者是对外**稳定契约** |
| G-code 输出字节 | golden 基线（`tests/golden/`，42 个文件）：`NCTOOL_UPDATE_GOLDEN=1 cargo test` 刷新，**必须人工 diff 复核**后再提交 |
| 架构 / 模块职责 / 数据流 | `docs/ARCHITECTURE.md` |
| 机床配置键 | `docs/MACHINE_CONFIG_GUIDE.md`（键清单、未知键告警） |
| 新增内置模板 | golden 用例 + `docs/PROCESS_CHECKLIST.md` 登记，并声明未经工艺评审 |

### 提交与分支

- 主分支 `master`。commit message 请写清**动机**（why），每条 commit 自包含（可独立编译、独立测试通过），
  便于 `git bisect` 与 `git revert`。
- 0.x 阶段：master 上直推 + 事后 review；1.0 后启用 PR 流程（1 个 approve + 三平台 CI 全绿），
  详见 [docs/RELEASE.md](docs/RELEASE.md)。CI 中 `coverage` job 非阻断，其余必须绿。

### 报告问题

- Bug / Feature → [issue 模板](https://github.com/lzg9698-code/nctool-tpl/issues/new/choose)（Bug / Feature 两类）。
- 用法、配置、模板写法疑问 → [Discussions](https://github.com/lzg9698-code/nctool-tpl/discussions)。
- 工艺 / G 代码安全性 → [《工艺核对清单》](docs/PROCESS_CHECKLIST.md)，**不要**当作工具 bug 提 issue。

## 相关文档

| 文档 | 内容 |
| --- | --- |
| [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md) | **贡献指南完整版**：环境搭建、质量门、测试矩阵、提交与发版流程、审查清单 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | **系统架构与设计说明**：三层 crate 架构、核心模块职责、数据流、错误模型、关键设计决策、扩展点。改动架构时请同步更新 |
| [docs/ROADMAP.md](docs/ROADMAP.md) | **开发路线与执行跟踪**：阶段划分、任务清单、交付物、排期、里程碑、风险登记、MVP 裁剪策略 |
| [docs/MACHINE_CONFIG_GUIDE.md](docs/MACHINE_CONFIG_GUIDE.md) | **《机床配置指南》**：配置键清单、内建预设、nctool.toml 自定义、加载顺序、`line_number_digits` 夹紧到 32 的内存安全理由、未知键告警等 |
| [docs/TEMPLATE_WRITING_GUIDE.md](docs/TEMPLATE_WRITING_GUIDE.md) | **《模板编写指南》**：NC/数学过滤器、必选/可选判定、引用机床配置、多模板 include/extends、validate/render 校验分层、反模式与发布前清单 |
| [docs/RELEASE.md](docs/RELEASE.md) | **迭代节奏约定**：版本号策略、发布节奏、提交流程、兼容性窗口、决策机制、Backlog 加权打分 |
| [docs/UI_ACCEPTANCE_CHECKLIST.md](docs/UI_ACCEPTANCE_CHECKLIST.md) | **Web UI 手工验收清单**（37 项可勾选）：端到端全链路 / 移动端 ≤480px / 校验定位 / 前后端逐字节一致 / 机床切换 / 主题 |
| [docs/REAL_PART_WALKTHROUGH.md](docs/REAL_PART_WALKTHROUGH.md) | **真实零件场景走查**（E5）：简化法兰盘的「端面+4 孔+切断」多工序演示与多机床对比，暴露行号续编/错误聚合/参数继承的局限 |
| [docs/PROJECT_STATUS.md](docs/PROJECT_STATUS.md) | **当前状态快照**：各阶段完成情况与测试计数 |
| [docs/PROCESS_CHECKLIST.md](docs/PROCESS_CHECKLIST.md) | **工艺核对清单**（阶段 A1）：内置模板 × 机床预设逐行核对结论、发现项 F1–F5、外部工艺评审待办 |
| [docs/DEV_PLAN_CLI_UI.md](docs/DEV_PLAN_CLI_UI.md) | CLI + Web UI 的设计细节（命令面 / API 契约 / 技术决策）。**其 §7 阶段计划已被 ROADMAP 取代** |
| [CHANGELOG.md](CHANGELOG.md) | 版本演进记录 |
| [core/README.md](core/README.md) | `nctool-core` 独立说明（数据模型 / 校验 / 注册表 / 生成管线） |
| [ui/index.html](ui/index.html) | Web UI 单文件前端设计与后端 API 契约 |

## License

MIT，见 [LICENSE](LICENSE)。
