# 模板编写指南

适用于 NCtool 2.2 工作台及 2.0 模板插件。模板由 MiniJinja 解析与渲染，通用能力和 NC 扩展分别启用。页面操作见 [WebUI 使用指南](WEBUI_USER_GUIDE.md)，完整动作字段见[系统设计](SYSTEM_DESIGN.md)与运行时 `nctool actions`。

## 普通模板的最小示例

在仓库根目录运行现有[文本渲染示例](../examples/v2/render.json)：

```bash
cargo run -p nctool-cli -- run template.render --input examples/v2/render.json
```

请求中的 `source` 是 Jinja 源码，`context` 是模板数据；输出在 `data.text`，可用 `--out hello.txt` 原子写出正文。若要在 Rust 中直接使用引擎，运行 `cargo run --example demo`，源码见[引擎示例](../examples/demo.rs)。

普通渲染不经过 NC 参数派生、机床注入或 G 代码后处理。对象、数组、字符串、数值、布尔和 null 均可作为通用上下文值。

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

参数预设是独立 JSON 资产，通过 WebUI 保存和加载，或用 `preset.*` 动作管理。它不会替代模板定义。

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

## NC 模板与扩展

先构建 `nc-bundle` 并启用 `nc` 组合，运行现有 [NC 示例](../examples/v2/nc.json)：

```bash
cargo run -p nctool-cli --features nc-bundle -- --profile nc run nc.generate --input examples/v2/nc.json
```

NC 请求使用 `params`，不是普通渲染的 `context`；机床通过 `machine` 或 `machine_id` 二选一指定。NC 自动注册数学与 NC 扩展、注入 `machine`，校验有效参数后进行格式化和后处理。

| NC 过滤器 | 用途 |
| --- | --- |
| `nc_fixed(3)` | 固定小数位输出 |
| `nc_signed(3)` | 带符号的固定小数位输出 |
| `nc_strip` | 去除数值尾部多余零 |
| `nc_pad(4)` | 整数补零，例如程序号 |

通用 `template.render` 若需要这些过滤器，必须在已启用提供方的前提下显式传 `extensions:["math","nc"]`；这只注册扩展，不自动取得 NC 校验或机床上下文。程序号和刀具号应定义为整数，不靠格式化截断错误数值。

保存 NC 资产时，在 `metadata.nc` 中声明领域规格和默认生成设置；工作台会据此匹配 NC 生成方式。示例：

```json
{
  "source": "O{{ program | nc_pad(4) }}\nG1 X{{ x | nc_fixed(3) }}\nM30",
  "schema": {
    "type":"object",
    "properties":{"program":{"type":"integer","minimum":1},"x":{"type":"number"}},
    "required":["program","x"]
  },
  "defaults": {"program":1},
  "metadata": {
    "execution": {"action":"nc.generate"},
    "nc": {
      "specs": [
        {"name":"program","kind":"integer","min":1},
        {"name":"x","kind":"number","min":-100,"max":100,"unit":"mm"}
      ],
      "options": {"line_numbers":true},
      "render_options": {"trim_blocks":true,"lstrip_blocks":true}
    }
  }
}
```

`metadata.execution.action` 表达工作台的执行意图；直接调用动作时仍应显式传递所需输入。`metadata.nc.options` 和 `machine_id` 是工作台绑定设置，不是所有直接 NC 请求的自动默认项。领域空白默认值来自 `render_options`，请求显式布尔值优先。

NC specs 支持类型、范围、整数、枚举、条件必选、派生和机床动态上界。其 `required` 是文档性声明；要求 JSON 字段必填应设置 Schema 的 `required`，实际模板引用和兜底也影响缺参校验。Schema 与 specs 同时校验，单位字段不做单位换算。

默认值优先级为用户参数、显式或根资产 defaults、specs defaults、模板内联 default；派生值覆盖同名输入。NC 参数支持数值、i64 整数、字符串、布尔和递归列表，不支持通用对象/null。NC 渲染保持严格字段策略，`lenient` 不跳过领域校验。

## 多工序与交付前检查

现有[多工序示例](../examples/v2/process.json)通过 `process.generate` 执行，公共参数被工序参数覆盖，行号跨工序续编；任一失败时整个正文不交付：

```bash
cargo run -p nctool-cli --features nc-bundle -- --profile nc run process.generate --input examples/v2/process.json
```

- 检查变量、Schema、默认值与 specs 的含义是否一致，覆盖缺参、错误类型和边界值。
- 检查静态/动态模板引用、空白、扩展条件和生成方式，保存后重读确认。
- 在选定机床上核对参数与输出选项，检查所有警告及最终正文。
- 按[工艺核对清单](PROCESS_CHECKLIST.md)完成实际 NC 资产复核；软件示例不作为加工程序。

旧版模板指南见[历史归档](archive/legacy/TEMPLATE_WRITING_GUIDE.md)。
