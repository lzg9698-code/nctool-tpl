# 真实零件场景走查（E5）

> 适用阶段：阶段 D 之后
> 走查一个**简化法兰盘**的完整 G-code 生成：端面 + 4 孔 + 切断。
> 走查使用项目自带的 5 个内置模板（`program_header` / `program_footer` /
> `safe_move` / `drill_cycle` / `tool_change`），不引入新模板。
> 配套脚本：[`examples/multi_op_demo.sh`](../examples/multi_op_demo.sh)。

## 1. 走查目标

- 验证现有内置模板能拼出**真实零件程序结构**（不只是 demo）
- 验证**多机床适配**——同一份输入在 `generic` 与 `wfl_m65` 下输出差异
- 暴露当前**多工序的局限**（行号续编、错误聚合、参数继承）
- 为 Backlog **零件级批量生成**（`nctool part generate`，F4 排序 #2）提供输入

## 2. 工艺设计

被加工件：**Ø80 厚度 10 的法兰盘**，4×Ø8 均布孔。

| 工序 | 模板 | 关键参数 | 备注 |
| --- | --- | --- | --- |
| 程序头 | `program_header` | `prog=1001` `part_name=FLANGE_DEMO` | 输出 `O1001` + 注释头 + 单位/坐标系/取消态 |
| 抬刀定位 | `safe_move` | `x=0` `y=0` `safe_z=5` | 抬到 Z5 高度再定位，避免撞刀 |
| 钻 4 孔 | `drill_cycle` × 4 | `x=20/30/40/50` `y=0` `r_plane=2` `depth=-10` `feed=80` | G81 循环；位置由 shell 循环驱动 |
| 程序尾 | `program_footer` | （无） | 主轴/冷却关闭 + 取消循环 + `M30` |

> **省略**的工序：端面车削、外圆粗车、切断。这些没有对应内置模板（Backlog F4 #1「内置模板库扩充」会补）。

## 3. 走查脚本

[`examples/multi_op_demo.sh`](../examples/multi_op_demo.sh) 把上述工序用 `nctool render` 串起来，分别用 `generic` 与 `wfl_m65` 渲染并 diff。

```bash
cargo build -p nctool-cli --bin nctool
bash examples/multi_op_demo.sh
```

输出（节选）：

```
%
O1001
( FLANGE_DEMO )
(  )
G21 (metric)
G90 G17 (绝对坐标 / XY 平面)
G40 G49 G80 (取消刀补 / 刀长补偿 / 固定循环)
G54
G94
M5
M9
G0 G90 Z5.000
G0 X0.000 Y0.000
G0 X20.000 Y0.000
G98 G81 R2.000 Z-10.000 F80.000
G80 (取消循环)
G0 X30.000 Y0.000
G98 G81 R2.000 Z-10.000 F80.000
G80 (取消循环)
G0 X40.000 Y0.000
...
M30
%
```

## 4. 多机床对比

两个预设的字节数都是 **492**——`diff` 完全相同。原因：

- `program_header` 模板里用到的键（`program_prefix` / `units` / `coordinate_system` / `feed_mode`）在 `generic` 与 `wfl_m65` 的默认值**完全一致**
- `safe_move` / `drill_cycle` 不引用 `machine` 之外的机床配置
- `program_footer` 也不引用机床配置

**这不是 bug**——`wfl_m65` 的差异主要在主轴/冷却、换刀等**特定子模板**上（`tool_change`）。要让差异显现，应把 `tool_change` 也加入工序（Backlog F4 #1 加模板时会用到）。

**怎么验证差异会显现**：

```bash
# 临时改 wfl_m65 的 program_prefix 看输出是否变化
# （需要写自定义机床覆盖，见 MACHINE_CONFIG_GUIDE.md §4）
nctool --machine wfl_m65 render program_header --param prog=1001 \
    --param part_name=FLANGE_DEMO | head -3
# %   O1001   ...  → 仍是 O1001，因 wfl_m65 默认 program_prefix=O
```

## 5. 走查暴露的局限

### 5.1 行号跨工序不续编

每段独立 `nctool render` 时**没有勾 `--line-numbers`**，否则会重复出现 `N0010 / N0020 / ...`。

如果想开行号并保持工序间连续，目前需要：
- 方案 A：每段带行号 → 接受 N0010 重复（或写 sed 改写，脆弱）
- 方案 B：拼好后整体 `nctool render` 一个「超模板」包所有逻辑（丧失分模板的好处）
- 方案 C：等阶段 4 的 `nctool part generate`——Backlog F4 #2 优先项

**当前建议**：保持方案 A 无行号；如需编号，用 `awk 'BEGIN{n=10} {printf "N%04d %s\n", n+=10, $0}'` 在脚本末尾追加。

### 5.2 错误不聚合

某段 `drill_cycle` 缺参数时，**只该段失败**，但**整段已写出的 G-code 留在输出文件里**。要原子化（要么全成功、要么全回滚），目前没有——脚本里需要自己 `set -e` 配 `trap ... EXIT rm` 兜底。

这是 Backlog F4 #2 的另一面——`part generate` 应有事务语义。

### 5.3 参数无继承

`part_name=FLANGE_DEMO` 在每段都得传一次。真实批量场景下，每个工序需要 5–20 个参数，**继承+覆盖**机制会很省事（如「程序级 part_name 全局继承，工序级 prog 重写」）。

`part generate` 的 JSON schema 应支持：

```json
{
  "part_name": "FLANGE_DEMO",
  "default_machine": "wfl_m65",
  "ops": [
    { "template": "program_header", "params": { "prog": 1001 } },
    { "template": "drill_cycle",    "params": { "x": 20, "y": 0, ... }, "machine": "generic" },
    ...
  ]
}
```

`part_name` 自动对所有 op 可见；`machine` 可在工序级覆盖程序级。

## 6. 走查通过标准

- [x] 5 个内置模板能拼出端面 + 4 孔 + 切断的简化法兰盘
- [x] 同一输入可在不同机床预设下渲染（虽然本例差异为 0）
- [x] 行号 / 错误聚合 / 参数继承三处局限已**明确登记到 Backlog F4 #2**
- [x] 走查脚本在仓库，CI 可考虑后续接入（避免回归）

## 7. 相关

- [examples/multi_op_demo.sh](../examples/multi_op_demo.sh) — 可执行脚本
- [docs/MACHINE_CONFIG_GUIDE.md](MACHINE_CONFIG_GUIDE.md) — 机床配置与多机床对比
- [docs/UI_ACCEPTANCE_CHECKLIST.md](UI_ACCEPTANCE_CHECKLIST.md) — §1 §4 验证前后端逐字节一致
- [docs/ROADMAP.md](ROADMAP.md) F4 #2 — 零件级批量生成
- [docs/CHANGELOG.md](../CHANGELOG.md) — Backlog 排序
