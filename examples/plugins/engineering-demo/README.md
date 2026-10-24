# 原生工作台通用示例

本插件只编辑点列表、显示折线并导出 JSON，不包含加工语义。它与 FTU 使用同一宿主视图、表格、场景、草稿、项目和任务机制。

需要 Python 3；将 plugin.json 中的解释器命令改为目标环境可用路径，再通过普通 plugins install/enable 安装启用并重启宿主。此目录须包含随包固定的 nctool_plugin.py，不依赖宿主隐式 PYTHONPATH。

包清单与 Scene2D 契约见 docs/ENGINEERING_WORKBENCH.md。几何/数值约束由动作 Schema 和插件负责；真实插件还应验证跨字段业务约束。
