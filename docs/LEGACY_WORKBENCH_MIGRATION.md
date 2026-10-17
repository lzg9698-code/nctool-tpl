# 旧固定工作台迁移说明（历史入口）

旧固定工作台及其离线作业迁移工具已退出本仓库当前维护范围，本页仅保留既有历史链接。现行 Rust 开发入口见[文档总览](README.md)。平台内置旧配置的只读检查和显式迁移继续保留，见[系统设计](SYSTEM_DESIGN.md#22-插件管理与工作区迁移)。

历史迁移说明按原路径 `docs/LEGACY_WORKBENCH_MIGRATION.md`，迁移脚本及测试按原路径 `scripts/dev/workbench_migrate.py`、`scripts/tests/test_workbench_migration.py` 收录于 `docs/archive/legacy/rust-nc-workbench.tar.gz`。该归档是当时版本，可能依赖当时的外部产品环境；不作为当前可执行指南。

[加工上下文恢复说明](archive/legacy/RUST_NC_RETIREMENT.md)提供 tar/manifest 核对方法；[旧固定工作台恢复说明](archive/legacy/WORKBENCH_RETIREMENT.md)保留其独立恢复包。恢复使用新的空目录，原有用户作业和验收记录不改写。
