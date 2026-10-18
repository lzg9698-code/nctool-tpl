> 历史记录：原路径 `docs/NC_REMOVAL_IMPLEMENTATION_2026-10-16.md`。保留当时日期、平台与结论，不代表当前源码已重新验收。当前能力见[项目状态](../../PROJECT_STATUS.md)，归档导航见[历史索引](../README.md)。

# 内置 NC 移除与 P0 验证记录

实施开始于 2026-10-16，最终签收于 2026-10-17，平台 Windows 11。提交基线为 `08ab29620a4fbf390517525e3c488b23c1bd35b1`，分支 `feat/shared-parameter-inputs`，结果对应包含未提交改动的工作树；未提交、推送或发布。范围见[开发方案](DEVELOPMENT_PLAN_2026-10-16.md)。

## 最终实现

- 活动源码删除 NC、机床、加工参数、刀具/工艺、同步粗车、越程槽、轨迹与加工工序/组合四组 Rust 插件，以及对应前端页面、专用编辑器、模型、样式、示例和加工验证脚本。
- 默认组合为 template + parameters，唯一可选 feature 为 math。数学须编译可用、配置启用并在请求 extensions 中显式选择；历史记录保存选择。
- 保留通用插件 SDK、运行时、版本化服务、外部进程协议与取消/超时/恢复、通用插件页面、资产集合、多文件 artifact 与原子交付。通用模板仍可生成任意文本，不对正文做内容分类。
- 删除 capabilities 中伪 NC 文档动作；NC 动作/服务/集合不能由别名外部插件接管。类似 machine-learning / processing.generate 的通用名字不受误拦。
- 旧 nc profile 拒绝启动；config check 只读，migrate 需显式 home 与 expected 指纹并备份原字节。未知 profile 不自动转换。旧加工标记（包括 metadata.nc=null）与实际加载引用不能降级渲染，历史产物可看可导出，退役动作不可重跑。
- 四个真实旧集合 nc、nc-parameters、sync-turning-assets、undercut-assets 在预检查与导入均整体拒绝，先于任何写盘；用户运行资产、草稿和历史不删除。
- 修复 Radix 对话框被原生 dialog CSS 隐藏、菜单定位叠加，以及“工作区资产”按钮仍使用旧 summary 样式的问题；按钮复用导航样式。没有进行 P1 功能扩展或 P2 工作台全面接线。
- CI 与发行构建改为 math/template；CI 覆盖默认与 math 测试，以及主流程、输入边界、共享参数/数学三组浏览器回归。

## 恢复材料与避免反复改动

实施前已冻结完整工作树，位于本地 `target/nc-removal-20261016/before`（418 文件），包含开始时已有未提交内容。随后从该冻结输入生成独立恢复包，未用旧 tar 覆盖活动源码；299 份非生成源码及逐文件哈希见[恢复说明](NC_REMOVAL_RECOVERY_2026-10-16.md)。既有恢复包保持原字节，新增归档测试逐成员校验本轮包。用户资产不在源码恢复包范围内。

先核对调用关系与分工，再删除实现，前后端收敛后统一生成资源。后续浏览器失败主要来自旧验收脚本预期：原先两个插件示例现仅保留数学、旧 NC 模板被隐藏、历史拒绝文案与 f64 文本显示为 3.0。按实际产品语义修正断言后重跑，未放松禁止降级或无部分交付条件。

## 实际验证结果

最终原始日志与截图在本地忽略目录 `target/nc-removal-20261016/`，不保证随仓库分发。下列均为本轮实际执行；数量由最终日志汇总，不采用历史脏工作树结果。

| 检查 | 实际结果与报告 |
| --- | --- |
| 默认 Rust | `cargo test --workspace --all-targets --locked --target-dir target/nc-removal-default`：268 项通过，日志 rust-default-final.log |
| math Rust | `cargo test --workspace --all-targets --all-features --locked --target-dir target/nc-removal-math`：268 项通过，日志 rust-math-final.log |
| 文档测试 | `cargo test --workspace --all-features --doc --locked`：3 项通过，doc-tests-final.log |
| 格式与 Clippy | fmt --all -- --check；workspace/all-targets/all-features/locked Clippy 且 -D warnings：通过，fmt-final.log / clippy-final.log |
| 最低 Rust | Rust 1.89.0 workspace/all-features/locked check：通过，msrv-final.log |
| 前端 | 锁文件安装后 lint、24 文件 146 项测试、正式 build_ui：通过，ui-test-nav.log / ui-lint-nav.log / ui-build-nav.log |
| 工具及恢复包 | Python unittest discover：30 项通过，python-final.log |
| 架构/资源/打包 | python scripts/check_v2.py：通过，architecture-nav.log；默认无数学、math 独立且无 NC 包/源码/chunk |
| 主浏览器 | browser_v2.cjs：通过，browser-main-nav.log；模板编辑、精度、草稿、保存冲突、外部报告插件、历史、集合往返、旧加工配置/模板/历史/包拒绝及响应式 |
| 输入边界浏览器 | browser_review_fixes.cjs：通过，browser-boundaries-final.log；非法输入、取消/失败、过期输出与插件流程 |
| 共享/数学浏览器 | browser_generic_parameters.cjs：通过，browser-shared-math-complete.log；14 场景，继承/覆盖/切预置/恢复跟随/取消预置/无效输入/依赖过期，以及数学显式扩展和重执行原历史字节不变 |
| 桌面预览 | 当前无 NC 导航；工作区资产按钮与邻项高度36px、padding6px、圆角7px、字号12px、颜色一致（当前视口）；菜单打开正常，无 console error / failed request |

默认与 math 均编译为独立固定二进制，浏览器均使用临时 home/workspace 与 loopback 动态端口。预览使用专用临时目录；没有借用用户运行资产。

## 依赖审计

已执行 `python scripts/audit_dependencies.py --output target/nc-removal-20261016/dependency-audit`：npm 成功且 0 漏洞，RustSec 下载超时，原始失败报告保留。随后通过 Git 独立获取新鲜 RustSec 数据库并执行 cargo audit --no-fetch，完成 Rust 审计，0 漏洞。

- RustSec 数据库提交：`479c888ce4401313bca61ba4aba2d0f6244c4a09`。
- Rust 告警保留：unsafe-libyaml 0.2.11 无维护；yoke-derive 0.8.3 被撤回。未忽略、批量更新或自动 audit fix。
- Cargo.lock SHA-256：`b2455599540323d7c1b2babd48b4228ec10f9fb57caf4cb4a8ad447d4f6ecd56`。
- ui/package-lock.json SHA-256：`eacd90fbc94c7afe2b2209601aa6986d055688cae57c62cf68aa2787b6c3cf3e`。
- 报告：audit-final-summary.json、cargo-audit-final.json、dependency-audit/ui.json。审计运行未改变锁文件。

## 剩余边界

P0 的本机实现和验证已完成，最终源码候选以本轮完整清单/快照签收。P1 的作者端共享绑定与受管历史恢复、P2 工作台入口统一仍待后续；没有因本轮清理提前宣称完成。

未运行目标最终提交的 Linux/macOS/Windows 远端 CI，未创建发行资产或下载签收，未提交、推送、打标签或发布。三平台与发行门槛继续属于 P3。依赖维护告警保留为明确待办；本机测试不代替这些工作。
