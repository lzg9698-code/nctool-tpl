# 机床配置指南

适用于 NCtool 2.2 工作台的 2.0 NC 插件。机床是工作区 `machines/<id>.json` 内的 JSON 资产，与宿主 `home/config.json` 分开。插件组合与目录见[系统设计](SYSTEM_DESIGN.md)。

## 启用 NC 能力

源码启动使用：

```bash
cargo run -p nctool-cli --features nc-bundle -- --profile nc ui
```

完整发行二进制可使用 `nctool --profile nc ui`。没有编译 NC 的纯模板二进制不能仅靠切换 profile 获得该能力。当前发行包没有旧版内置机床方案。

## 机床资产结构

```json
{
  "id":"demo_machine",
  "vendor":"Example",
  "model":"Virtual",
  "config": {
    "line_number_prefix":"N",
    "line_number_digits":"4",
    "program_prefix":"O",
    "max_spindle_rpm":"3000"
  }
}
```

这里是虚拟示例，不对应真实设备。`id` 必须与资产文件名/动作 `name` 一致，且是单个合法路径段。顶层只接受 `id/vendor/model/config`；`config` 是字符串键值表，数值约定也应写成字符串。不是旧版 TOML 或机床预设格式。

在 WebUI“机床配置”中新建，填写名称、厂商、型号及配置；常用字段通过表单编辑，其他约定可在配置 JSON 中编辑。保存冲突时重新读取并核对差异，不覆盖其他编辑者的版本。

## 动作读写与选择

| 动作 | 输入与行为 |
| --- | --- |
| `machine.list` | `{}`，列出已保存名称 |
| `machine.read` | `{name}`，返回 `machine` 与不透明 `fingerprint` |
| `machine.save` | `{name,machine,expected}`，新建 expected 为 null/缺省，修改带读取时的指纹 |
| `machine.remove` | `{name,expected}`，需要指纹 |
| `nc.generate` | 通过内联 `machine` 或已保存 `machine_id` 选择机床，不能同时指定 |

先用 `nctool --profile nc actions` 查看当前 Schema。将完整请求保存为 JSON 文件，用 `nctool --profile nc run machine.save --input <请求文件>` 等命令调用。

未指定机床时，NC 动作使用 `id=generic`、`vendor=Example`、`model=Virtual` 和空配置，后处理使用默认值；它不是已保存的机床资产，也不代表任何真实控制器。工序优先采用自己的机床设置，否则继承公共设置。

## 模板如何读取机床

NC 渲染注入 `machine.id/vendor/model` 和 `config` 内的键值。模板使用 `{{ machine.max_spindle_rpm }}`，而不是 `machine.config.max_spindle_rpm`。元信息覆盖 config 中同名键；配置值是字符串，需要数值运算时显式转换。

普通 `template.render` 不自动注入机床。NC 严格渲染会对未提供的机床成员报错；模板使用显式 default 或 is defined 时可表达可选配置，但不能因此省略设备必需的约定。

## 当前内置行为使用的配置键

| 键 | 行为 |
| --- | --- |
| `line_number_prefix` | 新增行号前缀，缺省为 `N`；显式值须非空白且不含控制字符 |
| `line_number_digits` | 行号补零宽度，缺省/解析失败为 4，有效宽度夹紧到 1–32 |
| `program_prefix` | 后处理识别程序号行的前缀，缺省为 `O`；显式值须非空白且不含控制字符 |
| `max_spindle_rpm` | 可供 specs 的 `max_from:"machine.max_spindle_rpm"` 读取动态上界 |

`config` 不采用旧版固定键名单；其他键由模板和 specs 约定。不会因为键保存成功就证明控制器支持其含义。动态上界缺失或非法时会警告并回退静态 `max`，需检查诊断。

## 生成选项

这些选项位于 NC 请求的 `options`，不是机床 config；由[后处理实现](../plugins/nc/src/postprocess.rs)定义：

| 字段 | 默认值与含义 |
| --- | --- |
| `format` | `gcode`；文本输出应使用 `template.render` |
| `line_numbers` | false，是否新增行号 |
| `line_number_step` | 10；0 按 1 处理 |
| `line_number_start` | 0，首个新增编号为起始值加步进 |
| `max_line_number` | 9999，超过上限/溢出后停止新增编号并产生警告 |
| `add_header_comment` | false，添加模板名头注释 |
| `strip_blank_lines` | false，移除空行 |
| `ascii_only` | false，非 ASCII 字符替换为 `?` |

程序号行与已有对应前缀行号的行不重复编号。多工序游标由宿主负责，工序不能通过 `line_number_start` 重置。Gcode 后处理会去除每行首尾空白，不能用普通文本结果代替最终 NC 输出核对。

保存模板的 `metadata.nc.options/machine_id` 可供文档工作台绑定；直接 NC 动作应在请求中指定 `options` 和机床。模板中的程序号位宽由 `nc_pad` 参数控制，不从机床配置自动推断。

## 迁移与核对

工作区导出在 NC 集合启用时包含机床资产；导入预检查拒绝同名冲突，不覆盖已有配置。直接修改 JSON 后应通过当前动作验证实际生成行为。

按[工艺核对清单](PROCESS_CHECKLIST.md)记录目标控制器、单位、坐标约定、行号与程序号规则。删除机床前检查使用该 ID 的模板元数据和工序草稿；当前机床删除动作不提供完整引用保护。

旧版机床指南见[历史归档](archive/legacy/MACHINE_CONFIG_GUIDE.md)。
