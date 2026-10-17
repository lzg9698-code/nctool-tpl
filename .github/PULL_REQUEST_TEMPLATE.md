## 改动摘要

<!-- 描述触发问题和改动后的行为；关联当前路线图或 issue -->

## 动机

<!-- 为什么改；关联的 issue、Backlog 项或发现项 -->

## 兼容性

- [ ] 无破坏性变更
- [ ] 有破坏性变更 → 已在 CHANGELOG `Changed` 节标注，并在下方写清迁移方式

## 验证（勾选实际执行项，注明平台与结果）

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo test --workspace --all-targets --all-features --locked`
- [ ] `cargo test --workspace --all-features --doc --locked`
- [ ] `cargo +1.89.0 check --workspace --all-features --locked`
- [ ] `npm run --prefix ui test` 与 `node scripts/build_ui.mjs`（前端改动）
- [ ] `python scripts/check_v2.py` 与源码快照测试（相关改动及发布验证）
- [ ] 三组 Chromium 验收（相关交互改动及发布验证，见 `docs/CONTRIBUTING.md`）
- [ ] 文档链接、接口和示例命令已核对（纯文档改动可注明其余检查不适用）
- [ ] `python scripts/audit_dependencies.py`（依赖变动或安全审计；附报告，注明未完成/发现项）

<!-- `--workspace` 不能省：根目录既是 workspace 根又是一个 package，
     不加它就只检查根 crate，其余资产 / SDK / 运行时 / 插件 / CLI 会被跳过。
     当前完整检查和浏览器运行方法以 docs/CONTRIBUTING.md 为准。 -->

## 测试

<!-- 列出相关正常、失败与边界验证；注明未执行的检查及原因 -->

- [ ] 正常路径 + 失败路径 + 边界均有覆盖
- [ ] 模板或产物变化已复核实际输入与最终输出（涉及生成行为时）

## 文档同步

- [ ] `CHANGELOG.md` 已登记（Added / Changed / Fixed）
- [ ] 涉及架构 / 模块职责 → `docs/ARCHITECTURE.md`
- [ ] 涉及模板写法 / 渲染扩展 → `docs/TEMPLATE_WRITING_GUIDE.md`
- [ ] 涉及共享参数 / 受管渲染 → `docs/PARAMETER_MANAGEMENT.md`
- [ ] 涉及 CLI / 动作 Schema / HTTP / 历史记录契约 → 测试与 `docs/SYSTEM_DESIGN.md`、`docs/PLUGIN_PROTOCOL.md` 同步
- [ ] 新增或归档文档 → `docs/README.md` 与历史索引同步

## 退役边界（改动涉及加工意图 / 资产包时必填）

- [ ] 不涉及
- [ ] 涉及旧加工元数据 / 冻结引用 / 旧历史 / 资产包：已保持拒绝降级、只读历史与混合包整体拒绝，
      未恢复任何内置 NC 动作、插件、页面或资源（见 `docs/PROJECT_STATUS.md`）
