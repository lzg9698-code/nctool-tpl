// Examples use the same action inputs as the workbench and never modify assets.
export interface SyntaxExample {
  id: string;
  category: string;
  title: string;
  description: string;
  source: string;
  input: Record<string, unknown>;
  note?: string;
  action?: string;
  plugin?: "math" | "nc";
}
export const syntaxCategories = [
  "基础与数据",
  "条件与循环",
  "过滤器与表达式",
  "引用与复用",
  "空白与注释",
  "插件语法",
];
export const syntaxExamples: SyntaxExample[] = [
  {
    id: "values",
    category: "基础与数据",
    title: "输出变量与嵌套数据",
    description:
      "双花括号输出数据。用点访问对象字段，用下标访问数组或特殊名称的键。",
    source:
      "你好，{{ user.name }}！\n第一个项目：{{ items[0] }}\n图号：{{ part['drawing-no'] }}",
    input: {
      context: {
        user: { name: "Ada" },
        items: ["轴", "套"],
        part: { "drawing-no": "A-001" },
      },
    },
    note: "参数面板中的名称要与模板变量一致。字符串是文本，数字参与计算；不要把数字写成带引号的字符串。",
  },
  {
    id: "default",
    category: "基础与数据",
    title: "可选变量与默认值",
    description:
      "未填写的可选变量可以用 default 指定替代文字。is defined 判断变量是否存在。",
    source:
      "{{ note | default('无备注') }}\n{% if email is defined %}邮箱：{{ email }}{% else %}未提供邮箱{% endif %}",
    input: { context: {} },
    note: "default 默认只替代未定义值。default('无备注', true) 也会替代空字符串、false、0 等假值；需要保留 0 时不要开启这一参数。",
  },
  {
    id: "null",
    category: "基础与数据",
    title: "空值与布尔值",
    description:
      "JSON 的 null 在模板里用 none 判断；布尔值用 true 和 false。空值与未定义是不同情况。",
    source:
      "{% if extra is none %}没有附加数据{% endif %}\n{% if enabled %}已启用{% else %}已停用{% endif %}",
    input: { context: { extra: null, enabled: true } },
  },
  {
    id: "if",
    category: "条件与循环",
    title: "条件、比较与组合判断",
    description:
      "if / elif / else 按条件选择文本。可组合 and、or、not，以及 ==、!=、>、>= 等比较。",
    source:
      "{% if qty > 0 and approved %}可以交付{% elif qty == 0 %}数量为零{% else %}待审核{% endif %}",
    input: { context: { qty: 3, approved: true } },
    note: "每个 if 都需要 endif。模板可以控制输出结构；加工规则仍由领域插件校验。",
  },
  {
    id: "for",
    category: "条件与循环",
    title: "遍历数组与循环序号",
    description:
      "for 遍历数组。loop.index 从 1 开始，loop.index0 从 0 开始；else 处理空数组。",
    source:
      "{% for item in items %}{{ loop.index }}. {{ item.name }} × {{ item.qty }}\n{% else %}没有项目\n{% endfor %}",
    input: {
      context: {
        items: [
          { name: "轴", qty: 2 },
          { name: "套", qty: 4 },
        ],
      },
    },
    note: "loop.first / loop.last 可判断首项与末项。循环结束用 endfor。",
  },
  {
    id: "items",
    category: "条件与循环",
    title: "遍历对象的键和值",
    description: "items 过滤器把对象转换为键值对，再用两个变量接收。",
    source:
      "{% for key, value in info | items %}{{ key }}: {{ value }}\n{% endfor %}",
    input: { context: { info: { material: "steel", batch: "B-01" } } },
  },
  {
    id: "set",
    category: "过滤器与表达式",
    title: "局部变量与简单表达式",
    description:
      "set 定义模板内的变量。算术可用 +、-、*、/；~ 把值转成文字并连接。",
    source:
      "{% set total = price * qty %}{{ '合计：' ~ total }}\n{{ '负责人：' ~ user.name }}",
    input: { context: { price: 12, qty: 3, user: { name: "Ada" } } },
    note: "模板内赋值不会修改参数表单。跨循环累加等复杂计算优先在插件中完成。",
  },
  {
    id: "filters",
    category: "过滤器与表达式",
    title: "常用文字与数组过滤器",
    description:
      "竖线把前一个结果交给过滤器。过滤器可以按顺序连接，也可以传入参数。",
    source:
      "{{ title | trim | upper }}\n{{ labels | join(', ') }}\n项目数：{{ labels | length }}\n{{ title | replace('report', 'summary') | trim }}",
    input: { context: { title: " report ", labels: ["A", "B", "C"] } },
    note: "lower 转小写，upper 转大写，trim 去除两端空白，join 连接数组，length 取长度。round 进行数值舍入，不保证固定小数位文字。",
  },
  {
    id: "macro",
    category: "引用与复用",
    title: "宏：复用一段输出",
    description:
      "macro 定义可复用的片段。调用时用括号传参数，endmacro 结束定义。",
    source:
      "{% macro line(name, qty) %}{{ name }} × {{ qty }}{% endmacro %}{{ line('轴', 2) }}\n{{ line('套', 4) }}",
    input: { context: {} },
  },
  {
    id: "include",
    category: "引用与复用",
    title: "include：插入其他模板",
    description:
      "include 插入另一份模板，默认共享当前上下文。示例试跑使用内存中的 header。",
    source: "{% include 'header' %}\n正文：{{ body }}",
    input: {
      context: { title: "检查报告", body: "已完成" },
      templates: { header: "标题：{{ title }}" },
    },
    note: "实际工作区中要先保存被引用的模板，使用其稳定标识（不是显示名称、文件路径或 JSON 文件名）。将 header 替换为你的模板标识。",
  },
  {
    id: "extends",
    category: "引用与复用",
    title: "extends / block：继承布局",
    description:
      "extends 选择基础模板，block 覆盖同名区域。示例基础模板为「报告 / content 区域」。",
    source:
      "{% extends 'base' %}{% block content %}你好，{{ name }}{% endblock %}",
    input: {
      context: { name: "Ada" },
      templates: { base: "报告\n{% block content %}待填写{% endblock %}" },
    },
    note: "base 的源码：报告（换行）{% block content %}待填写{% endblock %}。实际使用同样需要保存对应的基础模板。",
  },
  {
    id: "import",
    category: "引用与复用",
    title: "import：导入宏",
    description: "把其他模板中的宏导入到别名下，再通过别名调用。",
    source: "{% import 'macros' as m %}{{ m.label('Ada') }}",
    input: {
      context: {},
      templates: {
        macros: "{% macro label(name) %}负责人：{{ name }}{% endmacro %}",
      },
    },
    note: "macros 的源码：{% macro label(name) %}负责人：{{ name }}{% endmacro %}。import 默认不共享当前上下文；优先用宏参数传递需要的数据。",
  },
  {
    id: "comments",
    category: "空白与注释",
    title: "注释与原样输出",
    description: "{# … #} 是不参与输出的注释。raw 区域中的花括号按文字输出。",
    source: "{# 这里不会出现在结果中 #}开始\n{% raw %}{{ name }}{% endraw %}",
    input: { context: {} },
  },
  {
    id: "whitespace",
    category: "空白与注释",
    title: "控制空白与换行",
    description: "在标签边缘加 - 可以去掉相邻空白；它会同时删除空格和换行。",
    source: "A \n{{- value -}}\n B",
    input: { context: { value: "X" } },
    note: "本例输出 AXB。需要保留分行时不要随意添加 -；工作台的 trim_blocks / lstrip_blocks 属于独立渲染选项。",
  },
  {
    id: "math",
    category: "插件语法",
    title: "数学扩展与角度单位",
    plugin: "math",
    description:
      "数学扩展提供 sqrt、pow 和三角函数过滤器。带 _d 的三角函数使用度制，普通 sin / cos / tan 使用弧度。",
    source:
      "平方根：{{ 9 | sqrt }}\n平方：{{ 3 | pow(2) }}\n30° 的正弦：{{ 30 | sin_d | round(3) }}",
    input: { context: {}, extensions: ["math"] },
    note: '安装或启用插件不等于所有模板自动获得过滤器。普通 template.render 需要显式选择 extensions: ["math"]；NC 生成会选择 math 和 nc 扩展。',
  },
  {
    id: "nc",
    category: "插件语法",
    title: "NC 数值格式",
    plugin: "nc",
    action: "nc.generate",
    description:
      "nc_fixed 输出固定小数位，nc_signed 显式输出正负号，nc_strip 去掉多余尾零，nc_pad 补足数字位宽。",
    source:
      "X{{ x | nc_fixed(3) }} Y{{ y | nc_signed(2) }}\nO{{ program | nc_pad(4) }}\nF{{ feed | nc_strip }}",
    input: { params: { x: 12.5, y: 2.5, program: 42, feed: 100.0 } },
    note: "本例通过 NC 生成动作试跑。nc_fixed / nc_signed 会在指定精度把非零值变成零时拒绝输出；提高小数位后再生成。数值格式不能替代领域参数校验。",
  },
];
