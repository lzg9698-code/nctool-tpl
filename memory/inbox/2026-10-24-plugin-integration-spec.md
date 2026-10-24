---
by: Codex GPT-6
date: 2026-10-24
---

# 独立项目插件开发规范

- 用户要求提供规范，供另一个独立项目设计为 NCtool 插件。
- 新增 docs/PLUGIN_INTEGRATION_SPEC.md，并在 docs/README.md 增加入口；保留此前所有未提交改动。
- 以 cba817c 当前 SDK、runtime、CLI、WebUI 输入绑定及现有 Python 示例核对接口。推荐独立仓库、外部进程和薄适配层，协议 2；明确不支持第三方前端脚本和原生二进制 Artifact。
- 文档包含最小 plugin.json/Python 入口、保留命名空间、服务依赖、取消/超时、配置与数据目录、模板工作台、资产集合、安装和交付验收。
- 使用已有 Windows nctool 2.3.0 二进制，在仓库 target 下的临时独立 home/workspace 实测安装、启用、握手、中文动作与文本产物、非法输入及 out-dir；报告 target/plugin-spec-20261024/report.json。未重编译源码，未执行浏览器、完整协议故障注入或跨平台验收。
- 环境默认 python 为不可用 WindowsApps 别名；使用 Codex bundled Python。首次系统临时目录操作权限失败，改用仓库 target 下的临时目录后通过。
- 未实施另一个项目的适配，尚不知道其语言、业务契约及界面/二进制输出需求；未提交、推送或发布。
