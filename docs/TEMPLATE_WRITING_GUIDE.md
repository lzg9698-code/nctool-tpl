# 模板编写指南

适用于 NCtool 2.2 工作台及 2.0 模板插件。模板由 MiniJinja 解析与渲染；通用能力默认启用，数学扩展需显式编译、启用并在单次渲染中选择。页面操作见 [WebUI 使用指南](WEBUI_USER_GUIDE.md)，完整动作字段见[系统设计](SYSTEM_DESIGN.md)与运行时 `nctool actions`。

## 普通模板的最小示例

在仓库根目录运行现有[文本渲染示例](../examples/v2/render.json)：

```bash
cargo run -p nctool-cli -- run template.render --input examples/v2/render.json
```

请求中的 `source` 是 Jinja 源码，`context` 是模板数据；输出在 `data.text`，可用 `--out hello.txt` 原子写出正文。若要在 Rust 中直接使用引擎，运行 `cargo run --example demo`，源码见[引擎示例](../examples/demo.rs)。

普通渲染不经过任何领域派生、注入或后处理。对象、数组、字符串、数值、布尔和 null 均可作为通用上下文值。

## 保存为 JSON 资产

工作区内模板保存为 `templates/<id>.json`；ID 是一个合法路径段，引用时使用 ID，不加 `.json`。显示名称通过 `metadata.title` 修改，不改变 ID。

以下是模板资产内容，和动作请求中的包装字段不同：

```json
{
  "source": "Hello {{ user.name }}!{% for item in items %}\n- {{ item }}{% endfor %}",
  "tags": ["example", "report"],
  "schema": {
    "type": "object",
    "properties": {
      "user": {
        "type": "object",
        "properties": {"name": {"type": "string", "title": "姓名"}},
        "required": ["name"]
      },
      "items": {"type": "array", "items": {"type": "string"}}
    },
    "required": ["user"]
  },
  "defaults": {"items": []},
  "metadata": {"title": "问候报告"}
}
```

通过 WebUI 保存，或把 `{ "name":"hello", "asset":上面的资产对象, "expected":null }` 写成完整 JSON 请求文件，运行 `nctool run template.save --input <请求文件>`。新建时 `expected` 为 null/缺省；编辑现有资产先用 `template.read` 读取 `{name}`，保存时携带返回的 `fingerprint`。指纹是不透明字符串，不能自己拼接或用 null 强制覆盖。

保存后的渲染请求可以使用：

```json
{"template":"hello","context":{"user":{"name":"Ada"},"items":["templates","plugins"]}}
```

模板预设是独立的 JSON 资产，通过 `preset.*` 动作管理，用于保存某个模板的一组输入；它不是参数定义，也不替代模板的 `schema`/`defaults`。当前 WebUI 不再提供保存/加载预设入口。

## 参数定义与默认值

Schema 定义类型、必填字段、范围、枚举、嵌套对象和数组；工作台据此生成表单，后端也执行校验。“检查变量”只识别源码使用的变量及位置，识别后仍需明确选择参数类型。

普通 `template.render` 中，请求 `schema/defaults` 优先于根资产定义，显式 `context` 覆盖同名默认值，默认值按顶层合并。例如默认 `user` 对象不会与请求中的 `user` 递归补齐。Jinja 的 `default` 是模板内的另一层兜底。

JSON 入口保留支持范围内的整数；小数/指数使用有限 f64，不提供任意精度十进制运算。非零数下溢及超范围整数会失败，数值精度边界见[插件协议](PLUGIN_PROTOCOL.md#执行数值与领域校验补充2026-10-03)。

## Jinja 语法与模板引用

```jinja
{% if user is defined %}{{ user.name | default('匿名') }}{% endif %}
{% for item in items %}{{ loop.index }}. {{ item }}
{% endfor %}
```

工作台“语法提示”提供条件、循环、过滤器、宏、include/extends/import 和空白处理示例。使用项目实际支持的 MiniJinja 语法，模板有语法错误时先修复，再保存或生成。

引用已保存模板用 `{% include 'shared_header' %}` 等语法，名称是稳定 ID。临时请求也可提供 `templates` 对象，键为引用名、值为源码。静态引用会参与分析和删除保护；动态引用由实际渲染解析，不能仅凭变量分析通过判断执行成功。

`template.inspect` 的 `include_snapshot:true` 返回冻结的源码集合与根资产元数据；随后传 `snapshot` 和 `context` 渲染，不再读取资产。`snapshot` 不能与 `source/template/templates` 混用。空白选项 `trim_blocks`、`lstrip_blocks` 需显式指定，生成前检查结果换行。

## 渲染扩展

数学是当前唯一随发行提供的内置可选扩展，以独立 `math` feature 编译，不在默认构建中。使用数学过滤器需要三步都显式：

```bash
# 以 math feature 构建，或使用已包含它的完整二进制
cargo run -p nctool-cli --features math -- run template.render --input examples/v2/render.json
```

1. 编译：使用 `--features math` 构建，或使用完整二进制。
2. 启用：在 `home/config.json` 的 `enabled` 中加入 `math`；默认构建未包含 math 时会在启动时报错。
3. 单次选择：在渲染请求中显式列出 `extensions`：

```json
{"source":"{{ x | sqrt }}","context":{"x":9},"extensions":["math"]}
```

数学过滤器包括 `sin`、`cos`、`tan`、`asin`、`acos`、`atan`、`sqrt`、`exp`、`ln`、`log10`、`pow`、`floor`、`ceil`，以及以度为单位的 `sin_d`、`cos_d`、`tan_d`、`asin_d`、`acos_d`、`atan_d`；非有限结果（NaN/Inf）会失败。扩展选择进入输入签名与历史重放；未编译、未启用或未显式选择时不注册任何数学过滤器。领域过滤器不属于内置范围，领域能力应通过通用插件机制贡献。

模板资产只保存通用字段（`source`、`tags`、`schema`、`defaults`、`metadata`）。`metadata.title` 用于显示名称；参数定义和默认值分别在 `schema` 与 `defaults` 中，见[模板参数指南](PARAMETER_MANAGEMENT.md)。

带旧加工元数据（如 `metadata.nc` 或加工 `execution.action`）的资产会被识别并拒绝，不会降级为普通渲染。`metadata.execution.action` 仍可表达工作台对当前已安装通用动作的默认执行意图。

## 交付前检查

- 检查变量、Schema、默认值的含义是否一致，覆盖缺参、错误类型和边界值。
- 检查静态/动态模板引用、空白选项、扩展条件与生成方式，保存后重读确认。
- 使用数学过滤器时确认已显式编译、启用并选择；核对非有限结果与输入范围。
- 交付成功后核对正文、诊断与下载内容一致；软件示例不作为生产正文。

旧版模板指南见[历史归档](archive/legacy/TEMPLATE_WRITING_GUIDE.md)。
