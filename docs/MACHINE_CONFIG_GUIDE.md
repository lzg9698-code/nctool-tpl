# 机床配置指南

> 适用版本：nctool-cli ≥ 0.2.1 / nctool-core ≥ 0.2.1
> 模板通过 `{{ machine.<key> }}` 引用配置项；本指南说明**键清单、预设、自定义、加载顺序、关键约束**。

## 1. 机床配置是什么

`MachineConfig` 是一组「G/M 码编程约定」的命名集合，模板里用 `{{ machine.coolant_on }}`、`{{ machine.program_prefix }}` 之类引用，切换机床配置即可让同一份模板适配不同控制器。

每个 `MachineConfig` 由两类信息构成：

- **标识字段**（`id` / `vendor` / `model`）：用于 CLI/UI 展示、配置文件分组
- **配置键值对**（`config: BTreeMap<String, String>`）：实际被模板引用的约定

配置键是**字符串→字符串**的简单键值对。值为整数时仍按字符串存储，模板引用时通过 `| int` 显式转换（如 `{{ machine.line_number_digits | int }}`）。

## 2. 内建预设

nctool 内置 3 套预设，由 `core/src/machine.rs` 的 `MachinePreset` 给出，覆盖通用/铣车复合/车铣复合三类常见场景。

| 标识 | 厂商 | 型号 | 适用场景 |
| --- | --- | --- | --- |
| `generic` | Generic | CNC | 通用 FANUC 风格，作为缺省与新机床的参照基线 |
| `wfl_m65` | WFL | M65 | 铣车复合（Mill-Turn），主轴/副轴多通道控制 |
| `index_ms40` | INDEX | MS40 | 车铣复合（Turn-Mill），适合棒料/主轴同步 |

查看完整配置：

```bash
nctool machine show wfl_m65
nctool machine list              # 列出全部预设
```

**预设不可修改**。需要个性化时，要么在 `nctool.toml` 里新建自定义机床，要么改自定义机床的键值。

## 3. 配置键完整清单

源定义在 `core/src/machine.rs::KNOWN_CONFIG_KEYS`。下面是**完整列表**（按用途分组）。

### 3.1 程序与行号

| 键 | 类型 | 内置默认 | 用途 |
| --- | --- | --- | --- |
| `program_prefix` | String | `O` | 程序号前缀（FANUC 风格 `O` / Mazak 风格 `P` 等） |
| `program_digits` | Integer | `4` | 程序号位数（`{{ prog \| nc_pad(machine.program_digits \| int) }}`） |
| `line_number_prefix` | String | `N` | 行号前缀 |
| `line_number_digits` | Integer | `4` | 行号位数（**已夹紧到 32 位上限**，详见 §6） |

### 3.2 坐标系与单位

| 键 | 类型 | 内置默认 | 用途 |
| --- | --- | --- | --- |
| `coordinate_system` | String | `G54` | 工件坐标系（G54–G59） |
| `units` | Choice(`metric`, `imperial`) | `metric` | 单位制（→ G21/G20） |
| `feed_mode` | Choice(`G94`, `G95`) | `G94` | 进给模式（每分/每转） |

### 3.3 运动指令

| 键 | 类型 | 内置默认 | 用途 |
| --- | --- | --- | --- |
| `rapid` | String | `G0` | 快速移动 G 码 |
| `linear` | String | `G1` | 线性插补 G 码 |
| `clockwise_arc` | String | `G2` | 顺时针圆弧 |
| `ccw_arc` | String | `G3` | 逆时针圆弧 |

### 3.4 主轴与冷却

| 键 | 类型 | 内置默认 | 用途 |
| --- | --- | --- | --- |
| `spindle_on` | String | `M3` | 主轴正转 |
| `spindle_off` | String | `M5` | 主轴停止 |
| `coolant_on` | String | `M8` | 冷却开 |
| `coolant_off` | String | `M9` | 冷却关 |

### 3.5 程序与换刀

| 键 | 类型 | 内置默认 | 用途 |
| --- | --- | --- | --- |
| `program_end` | String | `M30` | 程序结束（M30/M99 等） |
| `tool_change` | String | `M6` | 换刀 M 码 |

### 3.6 元信息（文档/自检用，模板可引用）

| 键 | 类型 | 内置默认 | 用途 |
| --- | --- | --- | --- |
| `max_spindle_rpm` | Integer | `6000` | 主轴最高转速（供文档/用户自检） |
| `machine_type` | String | `generic` | 机床类型标识（`generic` / `mill_turn_multitask` 等） |
| `axes` | String | `""` | 轴配置描述（如 `"X Z C"`） |

## 4. 自定义机床

通过 `nctool.toml` 注册：

```toml
# 在项目或全局 nctool.toml 中
default_machine = "hero_x9"   # 可选：覆盖内置默认

[machine.hero_x9]              # 自定义机床的 id
id = "hero_x9"
vendor = "HERO"
model = "X9"

[machine.hero_x9.config]
program_prefix = "O"
program_digits = "4"
line_number_digits = "5"       # 5 位行号
linear = "G1"
rapid = "G0"
coordinate_system = "G54.1"    # 扩展坐标系
spindle_on = "M3"
coolant_on = "M8"
# 任何 KNOWN_CONFIG_KEYS 之外的键会被 `validate_config_keys` 告警
```

引用方式：CLI 用 `--machine hero_x9`，Web UI 在机床下拉里选。

**初始化示例配置**（不覆盖已有文件）：

```bash
nctool config init             # 在当前目录生成 nctool.toml
nctool config show             # 展示生效配置（来源路径 + 合并结果 + 警告）
```

## 5. 配置加载顺序

生效配置按**就近覆盖**原则合并，优先级（高→低）：

1. CLI/UI 显式指定：`--machine hero_x9` / Web UI 机床下拉
2. `nctool.toml` 的 `default_machine`
3. **项目配置**：从当前目录**向上递归**查找 `nctool.toml`
4. **全局配置**：
   - Windows：`%APPDATA%\nctool\config.toml`
   - Unix：`$XDG_CONFIG_HOME/nctool/config.toml`（无则 `$HOME/.config/nctool/config.toml`）
5. **内建预设默认值**（即上表「内置默认」列）

> 注意：项目配置与全局配置**合并**，不是覆盖。同一键项目优先；自定义机床集合取并集。

模板目录（`template_dir`）以同样规则覆盖。

## 6. 关键约束与已知坑

### 6.1 `line_number_digits` 被夹紧到 ≤ 32

该值用于格式化**每一行**的 G-code 前缀（`N` + 数字 + 空格）。若缺失上界，一个天文数字配置就会让**每行**去分配超大缓冲——而 Rust 的分配失败是**进程 abort、不可捕获**。因此实现上 clamp 到 `[1, 32]`（见 `core/src/pipeline.rs::MAX_LINE_NUMBER_DIGITS`）。

- `line_number_digits = "4"` → 每行前缀 6 字节
- `line_number_digits = "32"` → 每行前缀 34 字节
- `line_number_digits = "1000000000"` → **被夹紧到 32**，不会让进程崩溃

实测（ROADMAP E4.2）：10000 行 + 配 10 亿位宽 → 输出 231 KB / 1.81 ms，夹紧生效。用例见 `core/tests/large_program.rs`。

### 6.2 未知配置键 → 告警，不阻断

模板里拼错键名（如 `{{ machine.feed_modd }}`）不会编译报错——键缺失时回退 `default` 或 `UndefinedVariable`。如果**配置侧**给拼错的键也补了一个值（`feed_modd = "G94"`），两边的错误会互相掩盖、看起来"正常"。

`validate_config_keys` 在 `nctool machine show` 与配置加载时扫描，对**不在 `KNOWN_CONFIG_KEYS` 里的键**给出告警。**自定义机床可以合法携带未知键**（新机床的扩展约定），告警仅提示、不阻断，由人判断。

### 6.3 `Choice` 键值必须合法

`units` 仅接受 `metric` / `imperial`；`feed_mode` 仅接受 `G94` / `G95`。`nctool config show` 会把非法值列在「配置警告」里。

### 6.4 行号由后处理统一生成

行号是**逐行递增**的（`N0010 / N0020 / N0030...`）。已是 `N` 前缀的行、`O` 前缀的程序号行不会重复编号。`max_line_number` 触顶后停止递增但仍输出原行内容。

这是 `core/src/pipeline.rs::postprocess` 的契约——**不要把行号手动写进模板**，否则会出现双重编号。

### 6.5 非零步进：CLI 的 `--line-number-step`

CLI 的 `render` / `generate` 支持 `--line-number-step`（默认 10）。`step=0` 会被当作 1（否则行号原地不动，产出重复的 `N0000`）。

## 7. 验证清单

添加或修改机床配置后，建议跑：

```bash
nctool config show              # 看生效配置 + 来源路径 + 警告
nctool machine show hero_x9     # 看自定义机床的最终键值
nctool render drill_cycle --machine hero_x9 --param x=1 --param y=2 \
    --param r_plane=3 --param depth=-5 --param feed=100   # 实际渲染一次
```

`config show` 输出的「配置警告」段非空 → 说明有键拼错或值非法，必须处理。

## 8. 相关文档

- [docs/ARCHITECTURE.md](ARCHITECTURE.md) — 核心模块职责与数据流
- [docs/ROADMAP.md](ROADMAP.md) — 开发路线与验收状态
- [core/src/machine.rs](../core/src/machine.rs) — `KNOWN_CONFIG_KEYS` 与校验逻辑的权威定义
- [templates/demo_gcode.j2](../templates/demo_gcode.j2) — 可直接运行的示例模板
