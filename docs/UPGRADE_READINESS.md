# 1.0 API 升级就绪度

> 状态：**`nctool-tpl-v1.0.0` 候选 tag 已推送，CI 与三平台 CLI 构建通过；原生桌面人工验收仍未完成。**
> 记录日期：2026-09-30。尚未发布 crates.io，也未完成真实机床工艺验证。

## 当前判断

项目已从三个 0.x 公共 crate 的持续演进阶段进入 **1.0 候选准备阶段**。这次在冻结 API 前收紧了
两个已登记的缓存一致性接口，整合了 Web/Tauri 的业务执行路径，并修复了资产管理的并发覆盖、
损坏预设覆盖、默认机床误删等问题。工作区质量门和包内容检查通过后，可以评估是否冻结 API；
三个可发布 crate 的版本和内部依赖约束已同步为 `1.0.0`，根 crate 的发布包 dry-run 通过。
候选 tag `nctool-tpl-v1.0.0` 与提交 `4bd9466` 一致；该 tag 的 [CI](https://github.com/lzg9698-code/nctool-tpl/actions/runs/36700943120)
和 [Release workflow](https://github.com/lzg9698-code/nctool-tpl/actions/runs/36700947756) 均通过，三个 crates.io
发布步骤均跳过。GitHub Release 为预发布，包含 Linux、Windows、macOS CLI 二进制；没有上传 crate。

1.0 的产品定位仍是**G-code 模板开发工具**。语法通过、参数通过和 golden 测试通过，都不代表
NC 程序已适用于某台具体机床。内置预设和新导入模板仍须由工艺人员按机床手册复核并空运行，
再决定是否允许生产使用。

## 已完成的冻结前收口

- Tauri 命令现在是 `nctool_cli::server::route` 的适配器；模板、机床、预设、配置、lint、校验和
  渲染不再由 Web 与桌面两套 Rust 流程分别编排。每次调用重新加载配置，避免启动时快照过期。
- `core::asset` 是模板、机床、预设资产唯一写入路径。预设导入遇到损坏文件或输入重复名会拒绝；
  读文件前取写入指纹，写时冲突则保留并发更新。模板清单写入失败会作为部分成功警告返回，
  模板重命名会在删除源文件时再校验源指纹。
- 当前默认机床不能通过 CLI、HTTP/Tauri 删除。模板静态检查可以检查浏览器编辑器尚未保存的源码；
  重新加载源码时也刷新文件指纹。演示模式预设支持重命名和 YAML 导出。
- `unreviewed` 模板状态从清单暴露到 Web/API 和桌面类型；Web 列表有醒目标记，选中后渲染页显示
  工艺复核与空运行警告，避免用户把“可渲染”误认为“可投产”。
- `TemplateEntry.source_text` 私有化并提供 `source_text()` / `with_source_text()`；源码更新自动清除旧
  静态分析缓存。`derived_names` 与 `invalidate_analysis` 不再暴露为公共 API。迁移说明已写入
  [发布规范](RELEASE.md) 和未发布 [变更记录](../CHANGELOG.md)。
- 引入 Vite 7 系列修复前端依赖审计发现的开发服务器漏洞；模板归档导入报告明确标记所有导入项
  为 `unreviewed`，保留源路径和哈希。

## 本轮验证

| 验证 | 结果 |
| --- | --- |
| `cargo test --workspace --all-targets` | **1196 passed / 0 failed / 3 ignored**（1 个 registry cost spike，2 个 release-only 10k 行实测） |
| `cargo test --workspace --doc` | **5 passed / 0 failed / 1 ignored** |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo doc --workspace --no-deps` + `RUSTDOCFLAGS=-D warnings` | 通过 |
| MSRV | `cargo +1.89.0 check -p nctool-tpl -p nctool-core -p nctool-cli --locked` 通过 |
| 生产口径行覆盖率 | **92.37% / 92% 门槛** |
| `cargo audit --deny warnings` | 通过：Cargo.lock 无已知漏洞 |
| Tauri 桌面端构建 | `cargo build -p nctool-gui` 与 `cargo build --release -p nctool-gui --features custom-protocol` 均通过 |
| `npm run build` | 通过：TypeScript 检查与 Vite production build |
| `npm audit` | 0 vulnerabilities；前端安全审计已加入 CI 门禁 |
| Web UI 构建物同步、HTTP API / Tauri command 对拍 | 通过 |
| Headless Chromium Web UI 走查 | 通过：模板 create/save/lint-unsaved/derive/rename、unreviewed 标记、机床 new/delete、预设 rename/export/import、源码 reload/fingerprint refresh |
| 模板源静态检查 | 53 个 `.j2`，0 个解析错误，0 个 lint 发现；这不是工艺正确性证明 |
| NCTool V3 模板归档追溯 | 归档 CSV、源 ZIP 与报告中 28 个 SHA-256 一致，28 个目标路径均存在；发现 7 个静态 include/import 引用，目标全部存在。键槽模板的 3 处 include 路径改写与报告一致 |
| 发布包内容守卫 | 通过：75 个文件，无非库资产 |
| Markdown 链接与锚点检查 | 通过 |

## 正式 1.0.0 发布前仍需完成

1. 在支持的原生桌面环境按 [GUI 用户指南](GUI_USER_GUIDE.md) 和人工验收清单走查创建/编辑/冲突处理、
   默认机床保护、预设往返、保存 NC 文件和渲染错误反馈。当前完成的是 headless Chromium Web UI
   走查与 Tauri Rust 命令测试；Linux 容器没有可交互的桌面会话。
2. 若决定发布 crates.io，再按 `docs/RELEASE.md` 的依赖顺序显式手动运行发布 workflow。当前 tag 事件
   默认跳过发布；根 crate 的本地 `cargo publish --dry-run` 已通过，core/cli 的 registry dry-run 需等待
   上游 crate 已发布后再运行。
3. Windows / macOS CI 的 CLI 测试与 release 构建已通过，三个平台 CLI 资产已挂到预发布。原生 Tauri GUI
   的安装、启动与人工 UI 验收仍未完成；本轮 Linux 容器没有可交互桌面，不能以 CLI 二进制构建替代 GUI 验收。
4. 工艺人员仍需核对内置机床键值和计划用于实际加工的模板，并在目标机床空运行。项目应继续显著
   标注尚未复核的模板，不得把静态扫描结果升级成 `verified`。

## 1.0 冻结清单建议

- `nctool-tpl`：解析、错误类型、AST 变量提取、渲染器与过滤器；锁定 minijinja minor 并按文档迁移
  unstable AST API 风险。
- `nctool-core`：模型、校验报告/枚举、注册表元数据、派生规则、生成选项、资产写入行为和文件格式。
- `nctool-cli`：命令参数/退出码/JSON 输出、HTTP route/错误包络、预设 YAML 与配置文件兼容边界。
- `nctool-gui` 不发布到 crates.io；Tauri command 名称属于本仓库前后端契约，由
  `scripts/api_routes.json` 与 CI 对拍守护。
- 对每项 `Changed` 写明替换 API、文件格式兼容策略、CLI/UI 的错误行为和升级步骤；冻结后所有新破坏性
  需求进入 RFC / 下一个 major 规划。
