> 历史记录：原路径 `docs/NC_REMOVAL_RECOVERY_2026-10-16.md`。保留当时日期、平台与结论，不代表当前源码已重新验收。当前能力见[项目状态](../../PROJECT_STATUS.md)，归档导航见[历史索引](../README.md)。

# NC 移除恢复说明（2026-10-16）

本页说明内置 NC 能力移除后，如何核对与恢复**移除前**的源码上下文。本页只用于历史恢复与对照，不描述当前可执行操作，也不执行任何恢复。

当前产品状态见[项目状态](../../PROJECT_STATUS.md)；移除范围与验收见[开发方案](DEVELOPMENT_PLAN_2026-10-16.md)，本机实施与验证汇总见[NC 移除最终实施记录](NC_REMOVAL_IMPLEMENTATION_2026-10-16.md)。移除后的默认组合为 `template` + `parameters`，`math` 为唯一可选 feature，旧 `nc` profile 与内置 NC 插件、动作/服务、页面、示例与资源均已退出当前实现。

## 本轮恢复包

- 压缩包：[archive/legacy/rust-nc-removal-20261016.tar.gz](../legacy/rust-nc-removal-20261016.tar.gz)，811,113 字节，SHA-256 `60e467e62f2ab4cef19aea42d0b23bd7bc72b67b7e2e8c4d5ad210dfd1cfc5c5`。
- 逐文件清单：[archive/legacy/rust-nc-removal-20261016.manifest.json](../legacy/rust-nc-removal-20261016.manifest.json)，格式 `nctool-retired-rust-nc/1`，记录 299 个文件的原路径、字节数与 SHA-256。
- 记录基准 HEAD：`08ab29620a4fbf390517525e3c488b23c1bd35b1`（分支 `feat/shared-parameter-inputs`）。

该清单 `scope` 说明：包含移除前的完整工作树源码上下文（含当时未提交的修复），**不含**生成的前端资源、既有归档、长期记忆与被忽略的运行资产。它精确对应本轮移除动作前的源码状态，仅用于历史恢复。

## 旧恢复页

更早一轮（2026-10-14）的加工模块退役记录见 [archive/legacy/RUST_NC_RETIREMENT.md](../legacy/RUST_NC_RETIREMENT.md)，其压缩包为 `rust-nc-workbench.tar.gz`（205 文件、881,718 字节）。该页与两份归档保持**原始字节**，本轮不改写。

## 恢复方式（需自行执行）

1. 在新的空目录解压本轮压缩包，得到原路径文件。
2. 按 `rust-nc-removal-20261016.manifest.json` 逐文件核对字节数与 SHA-256。
3. 需要对照当时构建与依赖时，同时参考 `Cargo.lock` / `ui/package-lock.json` 的归档副本。

不要把归档解压覆盖正在使用的仓库，也不要用旧配置或锁文件覆盖当前精简版工作树。归档不包含已安装的 npm 依赖、Rust 构建产物或用户 home；需要运行旧版本时先核对当时的构建说明与依赖。本轮**未**执行任何恢复，也未提交、推送或发布。

## 数据与兼容

用户 `templates`、`parameters`、`.nctool/runs` 与浏览器草稿未删除。旧加工模板（`metadata.nc` 或加工 `execution.action`）及经 include/import/extends 实际加载的冻结来源会被拒绝，不会降级为普通渲染；旧加工历史只读，可查看、比较和导出原成功产物，但不能重跑或恢复为可执行文档；包含退役加工集合的资产包在预检查阶段整体拒绝。归档与恢复说明不构成加工工艺等价或机床验收。
