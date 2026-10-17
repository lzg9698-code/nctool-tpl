# 固定方案工作台归档退役与迁移验证

日期：2026-10-14。平台：Windows 11。基准HEAD：`e8fafd5`，含既有未提交/未跟踪内容。用户选择将旧工作台移出当前树并保留压缩包，明确不处理 `target/` 用户数据。本轮没有暂存、提交、推送或发布。

## 范围

- 归档并逐文件校验旧 Python 工作台、固定生成器、测试、冻结资源、预构建页面、React源码、独立构建/启动/验收入口，然后移出当前树。
- 共42个普通文件，压缩包SHA-256 `35f97872ab6e81885e4f6694722e80ac743fe8c06c474dd22ed80c15d64be7be`；压缩包、逐文件SHA-256与恢复步骤见[退役说明](../legacy/WORKBENCH_RETIREMENT.md)。额外保留当时的共享构建配置副本，当前主UI对应文件不覆盖。
- 原工作台全部为未跟踪文件，因此从Git状态表现为原未跟踪目录不再出现，不能用现有Git提交恢复。本轮归档是明确的恢复来源。
- 主 UI 仍按原规则排除旧目录，未重建或手改嵌入资源及 `source.sha256`。没有修改正式加工算法、数据库结构、用户数据、版本数字或依赖。
- 迁移工具与独立报告见[迁移指南](../../LEGACY_WORKBENCH_MIGRATION.md)。迁移只产生待确认输入，旧生成身份、程序及验收不继承；未映射字段保留在来源报告，无法保持原几何的场景拒绝。

## 本轮修正

前段开发中的迁移草稿曾错误地把左端键槽改绑到 `cap`，并给刀径、机床偏置填入示例值，合并A/B刀具角色。复核新工具源码确认：`left`键槽基准仍从图纸左端0开始，不能减17或改绑。早前13项迁移测试只验证了错误映射自身和结构契约，不能作为迁移正确性证据；本轮以修正后的独立预期与实际草稿消费为准。

最终迁移回归26项通过，另有归档3项；整套scripts测试94项全部通过。真实CLI读取、隔离HTTP保存/重开以及拒绝冻结均使用合成输入验证。旧代表性NC与新工序链的工艺等价、软件模拟、真实控制器与机床验收未完成。归档退役表示维护范围收束，不表示这些验收已通过。

## 实际检查

- `python -m unittest discover -s scripts/tests -p test_workbench_archive.py -v`：3项通过，核对全体成员字节/尺寸、旧入口不存在、恢复构建上下文及静态页面来源吻合。
- `python -I -X utf8 -m unittest discover -s D:/Dev/Projects/nctool/rustjinja/scripts/tests -p test_workbench_migration.py -v`：26项通过，覆盖精确槽位置/槽底、拒绝跨段与量化改变、来源双层指纹、畸形输入、无覆盖与失败回滚、新CLI/HTTP实际消费。
- `python -X utf8 -m unittest discover -s scripts/tests -p 'test_*.py' -v`：最终94项全部通过，包含上述29项；日志 `target/simplification/workbench-retirement-python-tests.log`。
- 本会话前段未改变主线源码时执行轴类276项全量通过；最终本轮未改其生产源码，不重复同一测试。最终scripts套件中的隔离HTTP/CLI回归直接消费现有主线。
- 实际解压到新建临时目录，全部42文件与清单SHA-256一致；临时副本已删除。归档9个Python源码编译语法通过，未运行归档服务。
- `node scripts/build_ui.mjs --check`：11个嵌入文件及来源一致性通过；没有重建。
- `python scripts/check_v2.py`：通过，核对纯模板依赖树、领域/工序服务边界、主UI与包内容。
- `git diff --check`：通过，Git提示既有文档的CRLF转换。

本轮失败的归档尝试来自自动审批服务超时；用户要求重试后成功，没有改变权限或绕过审批。归档脚本只移除已读入并与归档逐字节核对的路径，不包含 `target/` 用户数据。

本地日志位于 `target/simplification/`；恢复压缩包与清单在仓库 `docs/archive/legacy/`，二者用途不同。
