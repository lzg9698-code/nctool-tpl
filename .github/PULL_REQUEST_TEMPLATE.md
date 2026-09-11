## 改动摘要

<!-- 改了什么；对应 ROADMAP 的哪个阶段 / 任务项 -->

## 动机

<!-- 为什么改；关联的 issue、Backlog 项或发现项 -->

## 兼容性

- [ ] 无破坏性变更
- [ ] 有破坏性变更 → 已在 CHANGELOG `Changed` 节标注，并在下方写清迁移方式

## 质量门（提交前本地已跑通）

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test --all-targets`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`
- [ ] `cargo audit`

## 测试

<!-- 新增 / 修改了哪些测试；若刷新过 golden 基线，说明「输出为何变化」并确认已人工 diff 复核 -->

- [ ] 正常路径 + 失败路径 + 边界均有覆盖
- [ ] golden 基线：未变更 / 已刷新且差异已复核（说明原因）

## 文档同步

- [ ] `CHANGELOG.md` 已登记（Added / Changed / Fixed）
- [ ] 涉及架构 / 模块职责 → `docs/ARCHITECTURE.md`
- [ ] 涉及机床配置键 → `docs/MACHINE_CONFIG_GUIDE.md`
- [ ] 涉及模板写法 / 过滤器 → `docs/TEMPLATE_WRITING_GUIDE.md`
- [ ] 涉及 CLI 退出码 / `--format json` / Web UI HTTP 契约 → 测试与 README 同步

## 工艺安全（改动涉及模板 / 机床时必填）

- [ ] 不涉及
- [ ] 涉及 G/M 代码或机床预设：已声明**未经真实工艺评审**，投产前须由工艺人员逐行核对
      并在机床上空运行（见 `docs/PROCESS_CHECKLIST.md`）
