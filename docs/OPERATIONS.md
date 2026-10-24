# 历史、追溯与调用诊断

适用于当前 2.3.0 工作树。默认不自动清理历史，命令不会启动插件。历史归档与工作区资产包分别管理：历史归档包含原始输入、快照和产物；资产包包含当前启用插件的资产集合。两者都不代替运行配置、插件程序和外部环境的备份。

## 备份与恢复

```powershell
nctool --workspace D:/work/my-workspace history backup --out D:/backups/history.json
nctool --workspace D:/work/restored-workspace history restore --archive D:/backups/history.json
```

备份只收集可取得任务文件锁的已结束记录，返回 `records` 和 `skipped`；活动或锁定记录跳过，其他读取错误使操作失败。原 JSON 字节、未知扩展字段和数字字面量原样保留，轻量索引不作为权威备份来源。输出必须是新文件，不能放在活动的 `.nctool/runs/` 中。

恢复先校验完整归档、记录标识、状态、指纹和跨平台名称冲突，再独占创建一个新的工作区，逐文件按 create-only 原子写入并重建索引。既有目录一律拒绝覆盖。磁盘失败可能留下部分新目录及 `.nctool/restore-incomplete.json`，应保留现场并恢复到另一个新目录；宿主拒绝打开带该标记的工作区。标记只在全部写入成功后删除。

每包最多 10000 条记录、序列化后最多 256 MiB，单条最多 64 MiB。超过限制明确失败，不悄悄截断。归档指纹采用既有 FNV 变更检测口径，不提供来源认证；恢复不会启动或执行归档中的插件动作。

## 显式保留策略

```powershell
# 只读预览；固定截止时间为 Unix 毫秒，按自己的策略选择
nctool --workspace D:/work/my-workspace history prune --before-ms 1792713600000 --keep-latest 100

# 用预览返回的完整 fingerprint 确认；清理前必须写入新备份
nctool --workspace D:/work/my-workspace history prune --before-ms 1792713600000 --keep-latest 100 --apply "预览返回的fingerprint" --backup D:/backups/pruned-history.json
```

`keep-latest` 按开始时间降序、同时间标识升序保护最近的已结束记录；候选还必须在截止时间之前结束。活动和锁定记录不参与删除。应用时重新核对候选、参数和跳过集合，预览变化返回 `conflict`。删除前还会核对已备份的记录内容指纹，发现非协作改写时停止。从读取到备份、删除期间保持记录锁；备份失败不删除。删除按记录执行，后续文件系统故障可能部分完成，已完成部分可由备份恢复；不承诺跨文件事务。

## 历史查询规模

详情始终按标识读取原记录。查询仍扫描轻量索引，缓存最多 16384 份解析后的摘要；本机保存和删除使对应缓存失效，外部变更通过文件长度/mtime 检查发现。即使外部重写人为保留长度和 mtime，最多 2 秒后重新读取。缓存不用于资产写入、指纹确认或重执行输入。

缓存随文件删除回收，损坏摘要保留从原记录修复的路径。offset 分页在并发增删时不提供快照一致性；本轮没有引入数据库或游标格式。测量命令为 `python scripts/measure_history.py --label <名称> --sizes 1000,10000 --iterations 7`。

## 执行环境与隐私

新记录的 `environment.format_version` 为 2。新增 `provenance` 保存启动时观察到的宿主文件与安装包内容指纹、各插件的公开配置摘要；原声明版本、协议、服务绑定仍分别记录。旧格式返回 `unknown`，原历史不改写。

内容指纹沿用 FNV 变更检测，不作为签名或真实性保证；安装包排除解释器 `__pycache__`，不追踪包外解释器、外部文件或启动后的动态变更。内容未变不意味着外部行为或输出可重现。单次 CLI `run/actions` 不写历史，跳过这部分历史追溯采集。

配置默认不记录值，也不直接散列私密值。仅叶子 Schema 节点显式声明 `x-nctool-public:true` 的值参与公开投影；对象/数组分别递归，`writeOnly` 与凭据类字段名优先脱敏。未公开的值共用固定标记，因此私密配置改变不一定改变摘要。公开投影也只保存指纹，不保存原始配置。

```json
{"type":"object","properties":{"mode":{"type":"string","x-nctool-public":true},"token":{"type":"string","writeOnly":true}}}
```

## 调用诊断

在“插件与设置 → 调用诊断”刷新，或读取 `GET /api/v2/diagnostics`。它只保留当前宿主进程最近 256 次已完成调用：`trace_id/call_id/action/owner/parent_action/depth/started_at/elapsed_us/error_code`，重启清空。

后台任务以任务标识关联嵌套调用；直接动作生成独立关联标识，取消令牌跨 RPC 回调线程保留该关联。实际进入运行时的调用包括成功、Schema 拒绝、失败和被上层捕获的 panic，均有元数据记录；排队前拒绝的请求不算一次运行时调用。诊断不保存参数、配置、产物或错误正文，也不改变动作成功/失败语义。

## 资源与启动评估

当前哈希 JS/CSS 以 immutable 缓存复用，HTML 和 API 继续 no-store。静态 gzip 的离线体积与解压测量没有直接证明首屏收益，因此本轮不增加双份嵌入或运行时解压路径。实际前后测量与固定产物摘要见[项目状态](PROJECT_STATUS.md#第二轮优化与当前验证)。

资产规模可用 `python scripts/measure_assets.py --binary <固定release二进制> --sizes 100,1000 --repetitions 5 --output target/assets-scale.json` 测量。它只生成临时资产，分别检查目录、完整快照与 include 渲染，不接受用户工作区参数。
