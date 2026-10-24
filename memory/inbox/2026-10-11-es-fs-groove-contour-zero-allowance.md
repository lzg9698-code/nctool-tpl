---
by: Codex / GPT-6
date: 2026-10-11
---

用户进一步明确：ES和FS槽本身的轨迹部分不留余量。当前规则拆成外圆上限+0.500直径量、FS关联端面轴向单面0.100、ES端面零、ES/FS槽局部轮廓X/Z均零附加余量。槽整体定位仍由工序几何/基准决定，零加工余量不能被解释为取消刀具补偿或冻结全局位置。

独立规则和输出格式升级nctool-allowance-rules/0.2及nctool-allowance-targets/0.2；新增groove_trajectory声明及groove_trajectories结果。测试拒绝将0.1/0.5叠加到整段槽轮廓。原生产生成器及NC样例未修改。
