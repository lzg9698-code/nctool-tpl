> 历史记录：原路径 `docs/RESIDUAL_CLEANUP_PLAN_2026-10-14.md`。保留当时日期、平台与结论，不代表当前源码已重新验收。当前能力见[项目状态](../../PROJECT_STATUS.md)，归档导航见[历史索引](../README.md)。

# 残余清理范围与状态

初次调查日期：2026-10-14。状态更新：2026-10-15。原调查以 HEAD `e8fafd5` 及未提交工作树为基准；当时的文件计数、路径、建议和局部通过结果不代表当前工作树。本文替换已失效的“迁移必须保留”“需决定外部产品归属”及笼统完成表述。

## 当前范围

用户已授权清理旧资产与剩余问题，当前维护目标为本仓库最新 Rust 通用模板平台。仓库外产品、私有研究原型、设备工艺研究及旧离线作业迁移退出本仓库开发和验证入口；各自位置由使用者另行维护。

平台内置旧配置的只读检查、显式备份与 expected 指纹迁移继续保留。它与旧固定工作台的离线作业迁移工具是不同功能。退役标记拒绝、旧历史只读、混合包整体阻断及用户资产保留继续属于平台兼容边界。

## 已核对的历史资料

现有两份恢复包保留：

- `docs/archive/legacy/workbench-fixed-scheme.tar.gz` 与 manifest：42 个文件，旧固定工作台源码与入口。
- `docs/archive/legacy/rust-nc-workbench.tar.gz` 与 manifest：205 个文件，加工模块、旧示例、迁移工具和相关文档历史版本。

本轮核对了两份 tar 的压缩包哈希、成员路径、字节长度及逐文件 SHA-256，均与 manifest 一致。此次删除的研究提案、交付清单、审查提示和专用车削计划的历史版本已在第二份包中。归档版本早于本轮顶层文档修订，不能宣称其保存了本轮删除前的全部现行字节。既有 archive 历史和恢复包保持原字节，旧链接按当时目录解释。

历史入口见[归档索引](../README.md)、[加工模块恢复说明](../legacy/RUST_NC_RETIREMENT.md)和[旧固定工作台恢复说明](../legacy/WORKBENCH_RETIREMENT.md)。

## 本轮清理状态

| 范围 | 处理与证据边界 |
| --- | --- |
| 现行文档 | 根 README 和文档总览仅导航 Rust 平台；状态与路线图移除临时协作措辞，验证结论必须指向实际记录 |
| 研究与旧专题 | 删除 `NC_FAMILY_GENERATION_PROPOSAL.md`、`NEXT_GENERATION_ASSISTED_PROGRAMMING.md`、`SHAFT_WORKBENCH_V1_DELIVERY.md`、`REVIEW_PROMPT.md`、`SPECIALIZED_TURNING_PLUGINS_PLAN.md`；同步现行引用 |
| 有历史反向链接的顶层文档 | 精简方案、旧审查、机床/工艺/专用车削指南和旧迁移说明保留简短历史入口，退出当前导航，避免新增 archive 断链 |
| 旧离线作业迁移 | `scripts/dev/workbench_migrate.py` 与 `scripts/tests/test_workbench_migration.py` 按本轮决定移除，由工具清理工作负责；其历史版本已存档，当前文档不再给出运行命令 |
| 归档验证 | `test_workbench_archive.py` 与 `test_rust_nc_archive.py` 校验现有恢复包，继续保留；不因迁移工具移除而删除 |
| 其他源码与构建清理 | 由对应实现与验证记录说明，不从本页旧调查的样式行号或脚本状态推导通过 |

研究原型、图纸资料迁出及旧视觉脚本清理的早期过程见既有[WebUI 记录](2026-10-14-ui-refactor.md)。这些是历史事实，当前软件与依赖状态仍需本轮验证。

## 保留边界与未完成项

- 保留用户运行资产和历史；能力退出不清空 templates、parameters、machines、operations、runs 或浏览器草稿。
- 保留退役动作/模板拒绝、旧配置备份/指纹、取消/失败无交付及混合包整体拒绝。
- 保留全部既有 archive 文件原字节；恢复应使用新的空目录。
- 本轮已完成默认/math 测试、前端/浏览器、Clippy、MSRV、资源及依赖审计；旧迁移导入失败已随工具移除消除，Rust/npm 最终审计为 0 漏洞、Rust 无维护告警。实际命令与报告见[最终清理与验证记录](2026-10-15-rust-platform-cleanup.md)。原网络失败仍保留为历史证据。
- 拟发布提交的三平台 CI 与发行资产仍需实际证据。当前入口见[项目状态](../../PROJECT_STATUS.md)、[贡献指南](../../CONTRIBUTING.md)和[路线图](../../ROADMAP.md)。
