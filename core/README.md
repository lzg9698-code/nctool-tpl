# nctool-core

NCtool 核心库：在 `nctool-tpl`（模板解析 + 变量提取 + 渲染）之上，提供面向 G-code 生成的生产级能力。

## 能力

| 模块 | 说明 |
| --- | --- |
| **数据模型** | 参数值/参数规格（`ParamValue`/`ParamSpec`）、参数集（`ParameterSet`）、机床配置（`MachineConfig`）、刀具/零件/工序 |
| **参数校验引擎** | 渲染前校验必选参数齐全、类型匹配、默认值兜底，输出结构化校验报告 |
| **模板注册表** | 统一管理内存模板、文件系统模板、内置模板库，按分类筛选，携带参数规格 |
| **G-code 生成管线** | 校验 → 规格默认值兜底 → 上下文合并（参数 + 机床）→ 渲染 → 后处理（行号/注释/空行） |
| **机床配置层** | 内建 generic / WFL M65 / INDEX MS40 预设，模板通过 `{{ machine.xxx }}` 引用 |

## 快速开始

```rust
use nctool_core::pipeline::{GCodeGenerator, GenerationOptions};
use nctool_core::machine::MachinePreset;
use nctool_core::ParameterSet;

let g = GCodeGenerator::new();
let mut params = ParameterSet::new();
params.set_number("x", 21.0)
      .set_number("y", 15.0)
      .set_number("depth", -10.0)
      .set_number("feed", 100.0);

// 使用内置 drill_cycle 模板 + 通用机床配置
let machine = MachinePreset::Generic.config();
let out = g.generate("drill_cycle", &params, &machine, &GenerationOptions::default())?;
println!("{out}");
```

## 核心流程

```
参数集 + 模板名 + 机床配置
    │
    ▼
[1] 模板存在性检查
    │
    ▼
[2] 参数校验（渲染前）── 缺失/类型错误 → PipelineError::Validation
    │
    ▼
[3] 规格默认值兜底（未提供的可选参数自动填默认值）
    │
    ▼
[4] 上下文合并：params（裸值）+ machine（机床配置对象）
    │
    ▼
[5] 渲染模板
    │
    ▼
[6] 后处理：行号 / 头部注释 / 空行清理
    │
    ▼
G-code 输出
```

## 设计要点

- **渲染前可发现错误**：校验阶段定位缺失/类型问题，而非生成后才暴露
- **规格默认值兜底**：`ParamSpec.default` 在渲染前自动应用，校验与渲染保持一致
- **系统变量注入**：`machine` 是系统注入变量，校验时视为已提供，不要求参数集提供
- **一套模板适配多机床**：模板引用 `{{ machine.xxx }}`，切换机床配置即可

## 示例

```bash
cargo run -p nctool-core --example pipeline_demo
```

## 测试

```bash
cargo test -p nctool-core   # 52 单元 + 8 集成（workspace 总计 163）
```
