# Rust 工作台加工模块退役归档

日期：2026-10-14。依照 `docs/RUST_WORKBENCH_SIMPLIFICATION_PLAN.md`（历史文件，当前仓库未保留，原字节见恢复包）退出Rust工作台的全部内置加工能力。独立轴类工具和用户加工资产不在移除范围。

## 恢复资料

[压缩包](rust-nc-workbench.tar.gz)保留205个实施前源码、测试、示例、脚本与构建上下文文件，881,718字节。SHA-256：`8837a6fa9bb1167dc8558c45bd6f25a2fc9b16390f59aca5d27adf1fd30ace35`。对应[逐文件清单](rust-nc-workbench.manifest.json)记录原路径、字节数和SHA-256。

归档依据当前完整工作树而非仅Git HEAD：HEAD为 `e8fafd5e47a032d52f9f7a6adfbb384d454e0e15`，含当时未提交/未跟踪内容。归档包含四个加工crate、CLI/运行时/前端原上下文、Cargo/npm锁文件、示例与验证脚本、当前原文档。原有历史归档保持原字节。

移除四个加工crate：`plugins/nc`、`plugins/process`、`plugins/sync-turning`、`plugins/undercut`。同步退出它们的内置服务、页面、数据模型、业务算法和专用维护入口；数学与共享参数的通用功能继续保留。归档包含的通用上下文不表示当前实现也被移除，仅供核对当时依赖与调用关系。

## 恢复方式

在新的空目录解压，按清单逐文件验证。不要解压覆盖正在工作的仓库，不要把旧配置/锁文件覆盖到精简版。需要运行旧版本时先核对当时的构建说明与依赖；归档不包含已安装npm依赖、Rust构建产物或用户home。

完整工作树恢复快照（429文件、原Git状态与二进制补丁）另存本地 `target/rust-template-simplification/baseline/`；该快照是本地资料，不保证随仓库分发。压缩包是可随仓库保留的明确加工退役证据，本轮未提交/推送/发布。

## 数据与兼容

用户 `templates`、`parameters`、`machines`、`.nctool/runs`、本地NC资料和独立 `shaft_programmer` 数据未删除。旧标记模板保留查看，但不能自动降级成通用文本渲染；旧NC历史可查看原产物，重执行退役动作被拒绝。混合加工资产包不得静默跳过退役集合后部分导入。

旧配置需先检查迁移预览，再由用户显式应用，备份原配置并保留指纹冲突保护。当前命令与操作说明见[系统设计](../../SYSTEM_DESIGN.md)和[WebUI指南](../../WEBUI_USER_GUIDE.md)。归档保存和软件回归不代表加工工艺等价或机床验收。
