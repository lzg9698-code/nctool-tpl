# 融合基线

V3: 117 passed, 3 slow deselected（NC_NO_DIALOG=1 python3 -m pytest -m "not slow" -q）。

source-baseline.json 固定提交号、工作区修改和文件 SHA-256；source.patch 保留已跟踪修改。未跟踪功能由指纹清单和当前源工作区保留。template-mapping.json 记录全部模板的原文与适配指纹；原项目与原导入模板未被覆盖。

V3 的版权声明保留：Copyright © 2026 CNC Tools Studio. All Rights Reserved. 本次为用户授权的融合迁移，不改变源资产许可。所有迁移工艺保持 unreviewed。
