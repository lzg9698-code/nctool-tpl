> 历史记录：原路径 `docs/COMMIT_MESSAGE_CORRECTION_PENDING.md`。保留当时日期、平台与结论，不代表当前源码已重新验收。当前能力见[项目状态](../../PROJECT_STATUS.md)，归档导航见[历史索引](../README.md)。

# 提交说明修正待执行

本轮目标只更正提交信息，保留原提交文件树。远端目前仍为原历史，最新提交 515b58b。尚未执行新的 update-ref 或强制推送。

## 准确的提交范围

- 2ad8e95：后端 R1/R2/R5 实现、CLI交付与受管参数测试、CHANGELOG和四份审查/修复文档。没有修改或删除 ui/src/lib/managedParameters.ts，没有提交 cli/tests/retirement.rs，也没有提交协议文档的工作树修改。部分源码同时包含此前未提交的平台收敛变化。验证来自完整脏工作树，不能声称该提交独立检出通过。
- 7a2a1f3：前端修复及其所在的部分工作台重构、通用共享参数工具、样式和生成资源。scripts/check_v2.py 从52行扩充为104行；scripts/tests/test_check_v2.py为新增95行文件。不能将其描述为仅删除退役清单中的一个名字。保留通用 managedParameters.ts 并增加旧语义守卫，不是删除该文件。
- 515b58b：补交 cli/tests/retirement.rs，既包含此前未提交的配置/保留名回归，也包含本轮模板引用回归，共9项。测试依赖未提交的配套迁移实现，不能声称提交独立检出通过。

完整拟用信息与只修改Git对象/引用、校验工作区不变的脚本已写入 target/commit-message-correction/rewrite.py。脚本执行被自动审批拒绝，目前没有 result.json。

## 工作区事故

上一轮为恢复分支错误执行 git reset --hard backup-before-msgfix，覆盖了大量原有已跟踪但未提交的修改。备份分支只保存已提交内容，不能恢复这些工作树修改。已启动只读恢复调查；截至初步核对，仅证实部分差异有精确备份，不能保证全部可恢复。未自动覆盖当前工作区。

自动审批最后拒绝原因：Stage 2 classifier error - blocking based on stage 1 assessment。需要用户决定是否批准继续对象级提交信息更正，以及如何推进未提交内容恢复。
