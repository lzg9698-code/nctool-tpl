---
by: Codex GPT-6
date: 2026-10-24
---

# FTU NC 使用 Jinja 可行性分析

- 用户要求分析 NC 生成使用 Jinja 模板的可行性。本轮只分析与隔离验证，未改生产代码或将方案强行写入工程工作台计划。
- 建议采用业务计算/校验 → 版本化后处理模型 → 数值格式化 → 通用 Jinja 渲染 → 最终 NC 语义检查 → 编码换行定型 → SPF 交付。底座渲染仍领域中立，精车/控制器语义留在 FTU。
- 当前已有 template.inspect 快照、template.render 严格/Schema/空白控制和外部 host.call，无需另嵌引擎。模板 snapshot 不等于锁定插件程序；还需历史记录上下文/模板/后处理版本及生成策略。
- 用已有 Windows nctool 2.3.0 在临时隔离目录通过 8 项最小检查，包括外部 inspect→render 回调、Schema/缺变量失败、include 和资产变更后快照不变。报告 target/nc-jinja-feasibility-20261024/report.json；分析同目录 analysis.md。
- 实测模板末尾单换行被删除；SPF 的 ASCII/CRLF/无 BOM/末尾换行由 FTU 交付层处理。数值推荐由后处理器生成受校验的十进制字符串，不靠 round 假定输出精度。
- 渲染成功不等于 NC 正确；用户可改模板时需对支持子集解析/结构化校验，核对运动/进给/模态与图形模型，不声称换模板即适配所有控制器。
- 官方资料经 Context7 查询但无精确版本，随后核对 MiniJinja 2.24.0 标签源码；未以 latest 3.0.3 替代锁定版本。未验证真实 FTU 算法、输出解析器、控制器/实机、其他平台或源码重编译。
