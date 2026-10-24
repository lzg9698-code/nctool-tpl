# FTU 迁移为 NCtool 插件：现状与实施边界

> 本文是首次源码核对及复用设想。用户随后明确可换技术栈复现，当前实施方向见[通用工程工作台与 FTU 重实现计划](ENGINEERING_WORKBENCH_PLAN.md)，不再要求搬迁原 TypeScript/DOM 实现。

2026-10-24 只读核对：源项目为同级 `D:/Dev/Projects/nctool/FTU`，包名 `ftu-finishing-trajectory`，版本 1.0.0，TypeScript + Vite，浏览器原生 DOM 界面。本文保留首次核对及备选路线；用户随后允许规划通用底座扩展，当前实施方向为完整界面接入，详见[通用底座扩展与 FTU 实施计划](FTU_PLUGIN_PLATFORM_PLAN.md)。尚未实施插件或宿主扩展；现有测试记录不作为本次迁移验收。

## 已确认可复用的部分

| FTU 源文件 | 当前职责 | 插件迁移建议 |
| --- | --- | --- |
| `src/core.ts` | Input/Result、结构检查、轨迹和边界计算，导出 generateTrajectory | 保留 TypeScript 实现，编译成 Node.js 可调用模块 |
| `src/machining.ts` | 恒线速度、逐段进给及缺省值处理 | 与 core 一起复用，保留原有参数语义 |
| `src/sinumerik.ts` | generateSinumerik(Result) 生成 SPF 文本 | 作为受校验动作的导出步骤 |
| `src/project.ts` | 读取项目 JSON、恢复可编辑空值、补缺省字段 | 保留为编辑器项目读取逻辑，区分草稿与可执行输入 |
| `src/examples.ts` | 内外圆示例 | 作为插件示例和回归输入 |
| `src/main.ts`、`src/style.css` | 表格、交互图形、选择、播放、导入导出 | 拆出编辑器生命周期和导出工具，界面接入方式待确认 |
| `tests/*.test.ts`、`tests/browser/*` | 几何、工艺、项目、NC、浏览器验证 | 迁移时保留并补充插件边界验证 |

核心没有 DOM 依赖，不需要重写为 Rust/Python。NCtool 通过外部协议调用 FTU，不把精车模型引入根引擎、SDK 或通用模板插件；保留 FTU 独立开发和离线运行的能力。迁移初期保留原目录，不移动、删除原项目或覆盖它的 dist。

## 建议的插件边界

建议 ID 使用 `ftu`，动作使用 `ftu.` 命名空间，不占用已退役的 `nc.` 等身份。实际采用前仍需检查宿主中是否已有同名安装。

```text
FTU 独立项目
  业务核心：core / machining / sinumerik
  编辑器：原轮廓表格与交互图形
  Node.js 适配层：plugin.json + 双向 JSON-RPC
          ↕ 协议 2
NCtool：插件管理 / 任务 / 历史 / 文本产物
```

| 候选动作 | 输入 | 输出与约束 |
| --- | --- | --- |
| `ftu.analyze` | `{project: Input}` | `data.result` 包含轨迹、valid 与业务诊断；无效几何可返回分析信息，不附加工产物 |
| `ftu.generate` | `{project: Input, formats:[...]}` | 由当次 project 重算并校验成功后交付轨迹 JSON、CSV、SVG、SPF 中选定格式 |

这些是待实现的项目 API，不是宿主现有服务。动作提供输入输出 Schema；格式枚举及选项在实施时固定。若无需其他插件服务，requires 可以为空，无需为了接入而强行依赖模板渲染。

生成动作必须从输入重新计算，不能让用户提交任意 Result 并直接作为可信轨迹导出。若选择 SPF，应同时满足几何有效和 generateSinumerik 的精度/工艺检查；失败不交付部分成品。

JSON、CSV、SVG、SPF 都是文本，可以使用当前 Artifact。SVG 现从界面导出，若后台也要生成，需提取可复用的 SVG 序列化逻辑，并明确“当前视口”还是“完整轨迹视图”，不能让后台依赖 DOM。CSV 必须保留现有文本转义与防公式解释处理。SPF 保留原 ASCII、CRLF、无 BOM 和逐段进给语义。

建议 `data.replay_input` 保存实际规范化项目及所选格式，满足同一动作 Schema；补入的默认工艺参数显式反映在快照中，不能让历史重执行悄悄采用新默认值。记录 FTU 算法/数据版本，并保留原来“示例参数不是工艺推荐值”的说明。

## 不能直接复制的部分

**完整界面尚无现成插件入口。** 当前宿主只支持外部插件的 Schema 表单，不加载第三方前端脚本。FTU 的 main.ts 使用全局 `#app`、document 查询及 window resize 监听，不能直接导入 NCtool 页面；还需处理挂载/卸载、样式边界、事件清理、草稿和结果状态。

有两条可选实施路线：

1. **保留完整 FTU 界面（建议）。** 先设计通用的插件界面机制，再迁移 FTU 编辑器。宿主只掌握通用生命周期、动作调用、草稿和产物接口，业务界面仍由 FTU 管理。前端资源的加载、隔离、可信来源、接口及版本兼容需单独确定；现协议不能假装已支持。
2. **先交付计算插件。** 只实现上述外部动作，通过当前通用面板编辑结构化输入，先完成 CLI、任务、历史及产物联调。能够复用核心，但不能替代原来的交互轨迹工作台。

完整界面路线涉及宿主插件机制扩展，须在用户选择后形成具体接口设计；不能把 FTU 业务代码固定塞入通用模板页来绕过这一步。

另一个边界是空数值：FTU 的项目读取允许 null 恢复为 NaN，以保留尚未填写的草稿；RPC 不允许 NaN/Infinity。编辑草稿可保留未完成状态，执行输入必须完成有限数校验。不能把草稿解析器直接当作可执行输入校验器，也不能让非法值被 JSON.stringify 静默转成 null 后执行。

FTU 诊断为 `{severity,code,message,featureId}`，宿主诊断为 `{level,code,message,path?,template?,line?,column?}`。适配层需显式转换；featureId 保留在业务结果中，需要字段定位时映射到实际输入路径，不能作为未知字段混进宿主 Diagnostic。

Node.js 包建议编译后仅携带运行所需代码，不依赖开发时 node_modules 或相邻 FTU 目录的绝对路径。安装环境需要明确可用的 Node.js 路径/版本。同步几何计算会阻塞 Node 主线程；长计算应放到 worker，或选择经过实测的执行预算，保证接收循环还能处理 cancel，不能只有异步函数外形却无法协作取消。

## 实施顺序与验收

1. 冻结原项目当前源码与测试基线，确认未提交内容；当前源码可读，但本次沙箱中的 `git -C ../FTU status` 返回 Permission denied，未确认 FTU Git 状态。
2. 确定完整界面或先核心路线；在 NCtool 内的独立迁移目录准备副本，不写原 FTU。复用业务核心和用例，新增严格 Schema、诊断映射和 Node 协议适配层。
3. 用临时 home/workspace 验证安装、启用、握手、分析、导出、错误与恢复；结果与原核心逐项对比。
4. 按选择接入界面，保留轮廓编辑、图形定位、缩放平移、播放、保存重开和离线能力的明确范围；结果必须绑定当次输入，修改后标为过期。
5. 迁移并实际运行几何/工艺/项目/NC 测试、插件协议验收及浏览器测试；逐项记录 Windows 和其他支持平台的验证范围。

保留已有单位与坐标约定：内部 `{z,r}`、界面/NC 的 X 直径、mm、每转进给；保留有效性检查及各段工艺绑定。设备执行仍需独立验收，迁移通过不等于目标控制器、刀尖补偿或碰撞验证通过。

通用接口见[独立项目插件开发规范](PLUGIN_INTEGRATION_SPEC.md)，实际协议见[插件协议](PLUGIN_PROTOCOL.md)，故障验收见[插件作者验收](PLUGIN_ACCEPTANCE.md)。
