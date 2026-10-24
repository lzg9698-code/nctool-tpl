# FTU 独立插件

原生工程工作台的首个业务插件，使用 Rust 重新实现解析精车几何，NC 通过宿主 MiniJinja 服务生成。它不是宿主内置插件，不加入默认 workspace，不需要 Node/Python 运行时。

## 构建与安装

在 NCtool 仓库根目录运行（需要 Rust 1.89+、Python 3 用于打包）：

```powershell
cargo test --manifest-path external/ftu/Cargo.toml --locked
python scripts/build_ftu.py --release --out target/ftu-package
nctool --home ./demo-home --workspace ./demo-workspace plugins install ./target/ftu-package
nctool --home ./demo-home --workspace ./demo-workspace plugins enable ftu
nctool --home ./demo-home --workspace ./demo-workspace ui --port 0
```

打包输出目录必须不存在，脚本不覆盖旧包、不自动安装或启用。包中二进制与当前平台匹配，其他平台需各自构建。宿主必须包含 `nctool.package.json` 原生工作台支持；启用需要重启已有服务。侧栏入口为“精车轨迹”。

## 项目与交互

提供外圆/内孔、圆柱/锥面/端面、倒角/圆角、进退刀、方向/反向、逐段进给、恒线速度、简化毛坯和轴向禁入区。最多 500 个轮廓特征及 100 个禁入区。图形使用真实半径等比例绘制，纵轴刻度按直径显示。

表格可新增/复制/排序/删除，点击图形定位记录，支持缩放、平移、路径播放。正式输入需要显式工艺参数，示例值不是材料或刀具建议；不兼容原 FTU 的缺字段自动补值行为。原 FTU 代码和项目均不改动。

“保存项目”保存到工作区；“导出项目文件”保存带草稿状态的新版封装。未填数字和错误 JSON 可保存后继续编辑，不可生成；原始 FTU JSON 不是新版项目封装，不自动迁移。修改项目标识后用“另存项目”创建，不复用旧文件指纹。保存冲突保留当前草稿，可另存或重新打开磁盘项目。

## 动作

- `ftu.analyze`：`{project}` → `data:{result,scene}`，不返回产物。
- `ftu.template`：`{}` → `data.asset`，读取包内只读默认模板和上下文 Schema。
- `ftu.prepare`：`{project,formats,template?}` → `data.input`，冻结模板集合；亦接受已冻结输入。
- `ftu.generate`：`{project,formats,template?}` 或 `{project,formats,template_snapshot}` → 结果、产物及 `data.replay_input`。模板选择与快照互斥。

字段完整 Schema 和示例见随包 plugin.json。formats 为 json/csv/svg/spf 的非空不重复列表；生成由项目重算，不能提交伪造 Result。正式任务提交前宿主调用 prepare；CLI 直接调用 generate 也会冻结模板。SPF 生成依赖 template.inspect/render 服务版本 1，不要求 math。

## Jinja 后处理

默认模板不可直接修改；界面“编辑模板副本”可复制或编辑工作区模板，采用既有指纹保护。模板接收版本化上下文：start、cutting 和 blocks；数字已由后处理器格式化，不在模板中计算几何。

当前支持的输出块顺序为：`G18 G90 G710 DIAMON`、`G95`、`G96 S… LIMS=… F…`、逐段 G1/G2/G3 及 X/Z/可选 CR/F、最后 M17。可添加 ASCII 注释、空白、行号并调整同一块内的地址顺序；不能拆散初始化块、改变运动/进给/模态或添加未支持指令。渲染后逐块解析对照，失败不交付任何所选文件。

数字最多六位小数、半值远离零舍入、去尾零、规范负零、禁止指数。坐标和工艺值范围受限，舍入导致正参数归零、运动消失或圆弧失真时失败。SPF 为 ASCII、CRLF、无 BOM、末尾换行；SVG 保留解析圆弧，CSV 包含圆弧参数而不只含端点。

## 验证与边界

用解析算例、不变量和与旧 TypeScript 参考实现的差分验证；旧实现不是唯一正确判据。报告绑定宿主与插件哈希。涉及算法调整时运行独立 Rust 测试及 `scripts/browser_engineering.cjs`；后者需要先构建宿主资源和插件包，支持 NCTOOL_BINARY/NCTOOL_FTU_PACKAGE/NCTOOL_PYTHON 环境变量。

输出仍是理想接触点轨迹，不做刀尖补偿、刀杆/夹具碰撞或剩余毛坯仿真。主程序负责刀具、工件零点、主轴及起点定位，子程序不恢复调用前模态。几何、模板与输出检查不等于目标控制器或实机验收。
