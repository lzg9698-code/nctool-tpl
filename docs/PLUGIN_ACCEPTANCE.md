# 插件作者验收

`scripts/check_plugin.py` 使用实际宿主和协议，不需要浏览器或第三方 Python 包。它把插件复制到临时目录，使用全新的 home/workspace、随机 loopback 端口和测试配置，结束时关闭本轮宿主进程树。插件仍是同用户权限代码；测试计划应使用开发者准备的测试动作、测试配置和测试依赖。

```powershell
python scripts/check_plugin.py --binary target/codex-math/debug/nctool.exe --source examples/plugins/protocol-probe --plan examples/plugins/protocol-probe/acceptance.json --python C:/path/to/python.exe --output target/plugin-acceptance.json
```

先按[贡献指南](CONTRIBUTING.md)生成资源并编译对应二进制。Windows 没有可用 python3 时可用 `--python` 只修改临时副本的解释器命令。额外的本地测试提供方通过可重复的 `--dependency <插件目录>` 安装到临时 home；不下载或安装远程依赖。

## 测试计划

完整可运行样例在 `examples/plugins/protocol-probe/acceptance.json`，对应源码 `plugin.py`。这是测试专用插件，不装配进产品。作者可用自己的测试模式/测试构建编写同形计划，不必向生产动作添加等待或进程标识字段。

| 字段 | 要求 |
| --- | --- |
| `timeout_ms` | 500–10000 整数；设置临时安装描述的单请求预算 |
| `late_wait_ms` | 不小于 timeout_ms、最多 30000；给测试的迟到响应足够时间 |
| `identity_field` | 成功动作 data 中用于区分进程的字段，例如测试进程 PID；报告不保存该值 |
| `config` | 可选，写入临时 home 中本插件的配置，按插件的 config_schema 验证 |
| `success` | `{action,input,expected,callback_actions}`；expected 对 data 做递归子集匹配；callback_actions 按同一任务关联标识验证真实嵌套调用 |
| `invalid` | 明确违反输入 Schema 的 `{action,input}`，应在排队前或执行时以 invalid_input 拒绝 |
| `cancel` | 可协作取消的测试 `{action,input}` |
| `timeout` | 超过请求预算但随后回复的测试动作；不得把该次失败自动当作成功 |
| `unresponsive` | 忽略取消、持续占据槽位的测试动作，用于检验耗尽后的进程终止与重启 |

没有宿主依赖的插件可将 callback_actions 设为空，此项明确报告为不适用；有依赖的测试构建应提供实际调用路径。工具使用两个并发槽位，让单条迟到响应可以正常释放槽位，再通过两个放弃请求检验进程恢复。

## 验收结果

工具验证初始化/能力描述/依赖、成功输出 Schema、反向回调、非法输入、取消、超时、迟到响应的同进程复用，以及槽位耗尽后的新进程恢复。恢复探测使用计划中明确给出的独立测试请求，不自动重试可能有副作用的原业务请求。

成功报告保存检查名称、协议与二进制 SHA-256；失败报告保存错误码及已完成检查。两者都不包含输入、输出、配置或错误正文。任一断言失败返回非零，失败报告保留已完成的阶段；未执行项不算通过。CI 的 Linux 当前浏览器验收步骤后运行相同命令并上传报告。

本轮还使用临时复制的故障插件返回错误输出类型，验证工具会失败，而不是只验证正常样例。测试通过仅证明对应二进制、测试构建和输入计划；不证明任意插件业务正确，也不固定其外部副作用。
