# 融合基线

> 历史融合记录：原目录 `docs/fusion/`，记录旧版 V3 迁移。JSON 指纹清单和补丁按原字节保留；本文不构成当前模板导入说明。当前入口见[模板编写](../../../TEMPLATE_WRITING_GUIDE.md)和[历史索引](../../README.md)。


V3: 117 passed, 3 slow deselected（NC_NO_DIALOG=1 python3 -m pytest -m "not slow" -q）。

source-baseline.json 固定提交号、工作区修改和文件 SHA-256；source.patch 保留已跟踪修改。未跟踪功能由指纹清单和当前源工作区保留。template-mapping.json 记录全部模板的原文与适配指纹；原项目与原导入模板未被覆盖。

V3 的版权声明保留：Copyright © 2026 CNC Tools Studio. All Rights Reserved. 本次为用户授权的融合迁移，不改变源资产许可。所有迁移工艺保持 unreviewed。

## 归档引用说明

原文提及的 `source.patch` 为历史文件，当前仓库未以该文件名保留。现存补丁为 `rust-source.patch` 和 `v3-source.patch`；JSON 清单中的原路径不改写，不表示这些源文件仍存在于当前仓库。
