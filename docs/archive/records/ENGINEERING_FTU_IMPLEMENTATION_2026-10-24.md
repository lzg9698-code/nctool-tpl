# 原生工程工作台与 FTU 本地交付记录

日期：2026-10-24。工作基线 HEAD `cba817c3dcef7f9106466de15bd710b94be2a3b0`；本轮新增实现为未提交工作树改动，未发布。开始前已有文档和 memory 改动，未覆盖无关内容；源码恢复快照保存在本机 `.nctool-backups/engineering-20261024/`，构建前 UI 哈希为 `d26e6958857afa8f38e18c32e51633a692b4bddb54eb5ef294b2cf628543f543`。

## 实现

- 通用声明式工作台与包清单校验，宿主原生表格、场景、播放、诊断定位和会话撤销/重做。
- 项目与草稿持久化、指纹冲突、资产包导入/预检查/回滚；异步保存不会覆盖请求期间的新编辑。
- 去抖预览、预览并发限制、正式任务前冻结、历史及过期产物门禁；多文本产物整体 ZIP 下载。
- 独立 Rust FTU 插件，保留领域中立底座和默认 workspace；Jinja 后处理具有显式数值策略、上下文 Schema、冻结模板与渲染后语义比对。
- 一个无加工语义的折线插件复用同一工作台。原 FTU 源文件仅作参考和差分验证，未修改；其 Git 状态读取曾被环境权限拒绝，未声称原仓库干净。

使用及契约见[工程工作台](../../ENGINEERING_WORKBENCH.md)和[FTU 插件](../../../external/ftu/README.md)。

## 实际验证

| 检查 | 本轮结果 |
| --- | --- |
| Rust workspace/all-targets/all-features/locked | 307 passed，1 ignored；忽略项不计通过 |
| FTU 独立 Rust 测试 | 8 passed，含解析几何、反向/工艺绑定、禁入/毛坯与输出篡改 |
| 前端测试 | 47 passed；默认 fork 模式因沙箱临时文件失败，线程模式通过 |
| Python 脚本测试 | 60 passed |
| Rustfmt、Clippy、Rust 1.89 MSRV、doctests | 已执行并通过；宿主与独立 FTU 均包含在相应检查中 |
| 资源生成、--check、check_v2.py、diff --check | 通过；源码包枚举检查需在沙箱外只读运行 |
| 原有统一浏览器门禁 | 最终整体通过，13 + 9 + 8 项 |
| 工程工作台浏览器验收 | 11 项通过，含保存期间编辑、错误 JSON 恢复、模板副本、ZIP 实际解包和双插件复用 |
| 外部协议故障验收 | 6 项通过，包含取消/超时/迟到回复/进程恢复 |
| 原 FTU 对照 | 16 组内外圆、方向、反向、直线/圆弧进退刀的逐段几何与进给一致 |
| 依赖审计 | 宿主、独立插件、前端均通过既定门槛，所有锁文件未改动；保留下述公告 |

所有浏览器验收使用临时 home/workspace、随机 loopback 端口和本轮固定二进制，并校验每个嵌入资源。浏览器及安全公告下载受沙箱限制的运行未计通过，获准在沙箱外执行后形成证据。当前前端单测复现命令为 `node node_modules/vitest/vitest.mjs run --maxWorkers=2 --pool=threads`（工作目录 ui）。

## 边界、告警与异常记录

- Windows 本机调试构建：500 个特征、100 个禁入区、1004 个场景元素，四格式共 917162 字节；分析约 280 ms，文件导入及渲染约 1.9 秒。这是单机样例测量，不是跨平台或正式版性能保证。大段 CodeMirror 自动化 insertText 曾较慢，规模验收改为实际项目文件导入，未把虚拟 DOM 当作完整编辑器文档。
- 根锁文件中的 Tokio 1.53.1 保留 `RUSTSEC-2026-0386` informational/unsound 公告，涉及 AsyncFd I/O safety。报告保留完整内容，未将其抹掉或宣称不受影响；本轮未自动升级既有依赖。FTU 独立锁文件无该公告，npm 记录 0 个漏洞。
- 参数回归曾遇到一次历史读取 `path_escape`；独立重跑和随后统一门禁均通过。保留 previous-failure.json，未放宽路径守卫，也未把该偶发错误宣称已根因修复。
- Linux/macOS 及远端 CI 本轮未实际执行；已配置三平台独立插件构建/测试和产物上传、Linux 浏览器验收及 MSRV。
- 未进行控制器仿真、刀尖补偿、碰撞或实机验收；没有恢复内置 NC，也没有提交、推送、打标签或发布。

## 本机证据与开发包

完整索引为本机 [engineering-delivery.json](../../../target/engineering-delivery.json)，包括宿主/插件 SHA-256、源文件哈希、报告路径与性能数据。宿主 SHA-256：`b489c0109bf15cf66d03b4a882544551e2a2d558ddf1a2540b9c99494f1bb352`；插件 SHA-256：`5f557b7f9a48d796b0d1e4c8365a13c93e3448bcfe89e5d3f96eb8e06386902f`。

Windows 开发验收包为本机 [ftu-plugin-0.1.0-windows-x86_64-dev.zip](../../../target/ftu-plugin-0.1.0-windows-x86_64-dev.zip)，可解压后通过新版宿主 plugins install/enable 安装。它不是正式发行包。target 下的二进制、截图、日志和恢复快照均为本机忽略产物，不随仓库或正式发行保证提供。
