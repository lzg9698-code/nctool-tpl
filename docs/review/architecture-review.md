# nctool-core 架构与核心正确性审查报告

- **审查人**：高见远（架构师）
- **审查对象**：`D:\Desktop\nctool\rustjinja\core\src`
- **范围**：`asset/*`（atomic / guard / machine / path / preset / spec_fingerprint / template / mod）、`manifest.rs`、`registry.rs`、`model.rs`、`pipeline.rs`、`lib.rs`
- **代码量**：约 11,000 行（含单测）；非测试代码约 5,200 行
- **结论概要**：整体工程质量**高于同类项目平均水平**——错误类型设计、文档化的"为什么"、单测覆盖率、纵深防御意识都做得相当扎实。但存在 **1 个 P0**（并发写静默覆盖，与模块自述的设计承诺直接冲突）、**7 个 P1**、**23 个 P2**。最集中的风险面在"写通道的并发模型"（声称有锁、实际无锁）与"外部输入的读取上限"（三处无上限 `read_to_string` + serde_yaml 无别名预算）。
- **跨模块定稿**（已与 CLI 侧 reviewer 对齐，两份报告口径一致）：
  1. P0-1 的修复新增 `WriteError::LockBusy { path }`，CLI 归 **kind `write_conflict`（退出码 6）**，不新增 kind、退出码矩阵零改动；
  2. **撤回**"给 `create` 加 `O_EXCL`"的初稿建议——它会破坏 `template_duplicate`(6) / `name_conflict`(6) 的对外退出码契约，且会引入崩溃残留。P1-7 改由锁一并解决，**CLI 零改动**；
  3. P1-4 采用**降级方案**（`new()` / `Default` 签名不变 + `builtin_warnings()`），**CLI 零改动**；
  4. `registry.rs:399`（`add_file` 无界读）由本报告 P1-1 认领，与 CLI 侧同类问题**同源、不重复计数**；
  5. P0-1 的实施另加两条硬约束：**HTTP 侧 `LockBusy` 消息不得含绝对路径**（`server.rs:1411-1417` 的 `Conflict` 分支把 `path.display()` 写进响应体，属既有泄漏，不得照抄）；**服务侧必须用 `try_lock()`**（`serve_requests` 单线程 + tiny_http 无读超时，阻塞等锁会钉死 `nctool ui`）。

---

## 一、P0-致命（必须立即修复）

### 【P0-1】乐观锁是 check-then-write，全 crate 零处互斥原语 → 并发写静默互相覆盖

- **位置**：`core/src/asset/mod.rs:67-95`（`WriteKernel::write_guarded`）
- **问题**：宣称"绝不静默覆盖他人改动"的乐观锁，实际是「先读快照 → 比对 → 再写」的三步，**没有任何跨进程/跨线程互斥**，比对的结论在写入时已过期。
- **证据**：

```rust
// core/src/asset/mod.rs:72-88
let snapshot = guard::read_snapshot(path)?;          // T1：读快照
let actual = snapshot.as_ref().map(|(_, fp)| *fp);
if actual != expect { return Err(WriteError::Conflict { .. }); }   // T2：比对
...
if action != WriteAction::Unchanged {
    atomic::write_atomic(path, bytes)?;              // T3：写（T2 与 T3 之间无保护）
}
```

补充佐证：全 `core/src` 内 `flock` / `lock_exclusive` / `OpenOptions::create_new` / `LockFile` 检索结果为 **0 处**（唯一的 `create_new` 只出现在 `template.rs:104` 的注释里，说明改法但没改）。`guard.rs` 名字像"锁"，实际只是 FNV-1a 指纹计算模块（`guard.rs:53-62`）。

- **影响**：
  1. CLI 与 HTTP 服务（`server.rs` 的 `/api/presets` 与 CLI 写同一个 `presets.yaml`）并发运行时，两个写者**同时通过**比对，后 `rename` 者胜，**先写者的一整次编辑被静默丢弃**——且双方都收到 `Ok(Updated)`。
  2. 丢失的对象是机床参数 / 预设参数值，直接参与 G-code 渲染，属于"渲染成功但结果错误"这一类本项目零容忍的失效。
  3. 这是**设计承诺与实际实现的直接冲突**：`mod.rs:8` 写"任一不同即 `WriteError::Conflict`，**绝不静默覆盖他人改动**"，而 `template.rs:100-109` 的注释也自认"仍是 check-then-write……后 `rename` 者胜、静默覆盖"。注释承认 ≠ 风险消除。
- **改进建议**（代码级，已与 CLI 侧 reviewer 对齐定稿）：
  1. **引入同目录锁文件 + 操作系统级 advisory lock**（零新增依赖，跨平台）：
     ```rust
     // 新增 core/src/asset/lock.rs
     pub(crate) struct DirLock { file: std::fs::File }         // 持有即持有锁
     impl DirLock {
         pub fn acquire(target: &Path) -> Result<Self, WriteError> {
             let lock_path = target.with_extension("nctool.lock");
             let mut opts = std::fs::OpenOptions::new();
             opts.create(true).truncate(false).write(true);
             let file = opts.open(&lock_path).map_err(|e| map_io(e, &lock_path))?;
             // 阻塞式排他锁（有界等待 2s）：Windows LockFileEx / Unix flock(LOCK_EX)
             // 超时 → WriteError::LockBusy { path }
             lock_exclusive_timeout(&file, Duration::from_secs(2))
                 .map_err(|_| WriteError::LockBusy { path: target.to_path_buf() })?;
             Ok(DirLock { file })
         }
     }
     // 只 unlock、**不** unlink：Unix 上删除锁文件会让 flock 失效；
     // OS 在进程死亡时自动释放，故无需 stale 检测 / TTL。
     impl Drop for DirLock { fn drop(&mut self) { let _ = unlock(&self.file); } }
     ```
     然后 `write_guarded` 改为 **先取锁 → 读快照 → 比对 → 写 → 释放锁**（`Drop` 保证异常路径也释放）。`std::fs::File::lock()` / `unlock()` 自 Rust 1.89 已稳定，若 rust-version（`1.85`）不适应则退到 `fs2` / `file-lock` 一类 200 行以内的小依赖。
  2. **配套新增错误变体** `WriteError::LockBusy { path: PathBuf }`（`WriteError` 已 `#[non_exhaustive]`，新增变体不破坏下游匹配）。与 `Conflict` 的分工必须写进文档：**`Conflict` = 内容已被改（乐观锁，需以新内容为基线重试）；`LockBusy` = 另一进程正在写（互斥锁，稍后重试同一操作）**。二者都可重试，CLI 侧共用 kind `write_conflict`（退出码 6），**不新增 kind**，退出码矩阵 / README 表 / `exit_code_matrix` 测试零改动。
  3. **⚠️ HTTP 通道的消息不得含 `path`**（CLI reviewer 指出，已采纳）：`server.rs:1411-1417` 的 `Conflict` 分支把 `path.display()` 直接写进响应体，属既有的绝对路径泄漏。实施 `LockBusy` 时**不得照抄该形状**，HTTP 侧须脱敏为 `"预设文件正被另一个 nctool 进程写入，请稍后重试"`（409 + `write_conflict`），绝对路径只允许出现在 CLI / stderr 通道。此约束须写进 `lock.rs` 与 `asset/mod.rs` 的文档。
  4. **等待时长必须可注入，服务侧用 `try_lock()`**（CLI reviewer 指出，已采纳）：`serve_requests`（`server.rs:1272`）是单线程顺序循环，且 tiny_http 0.12 无读超时，HTTP 请求若在 `route()` 内阻塞等锁会**钉死整个 `nctool ui`**（GUI 复用 `server::route`，同样受影响）。故 `asset/lock.rs` 必须同时提供：
     - `DirLock::acquire(target, wait: Duration)` —— CLI 命令行传 2s；
     - `DirLock::try_lock(target)` —— 服务侧使用，拿不到锁立即返回 `LockBusy`，由前端决定重试。

     `LockBusy` 在 HTTP 侧本就极罕见（`nctool ui` 自身单线程），try-lock 不损害可用性。
  5. **锁不替代乐观锁**：锁解决"写-写交叠"（跨进程），`expect_hash` 解决"读-改-写"的 ABA（含跨会话：用户昨天取的指纹今天才提交）。两层保留，此句须写进 `lock.rs` 模块文档。
- **严重程度**：**P0**

> **定稿说明（已与 CLI reviewer 对齐）**：本报告初稿曾建议"最小改动走 `OpenOptions::create_new`（`O_EXCL`）"作为 `create` 的独立修复，**现予撤回**——理由见 P1-7。

---

## 二、P1-严重

### 【P1-1】三处无字节上限的 `read_to_string` + serde_yaml 无别名展开预算 → 内存放大 DoS

- **位置**：`core/src/manifest.rs:515`、`core/src/registry.rs:399`、`core/src/asset/machine.rs:335`
- **问题**：`preset.rs` 有 `MAX_PRESET_FILE_BYTES`（1 MiB，`preset.rs:46`）并做了流式 `Take` 截断，但清单 / 模板文件 / 机床配置三条读路径**完全没有上限**；同时 `serde_yaml 0.9.34` 的别名展开**没有预算**（`De::jump` 每遇一处 alias 就重新反序列化整棵子树），`unsafe-libyaml 0.2.11` 也没有深度/别名限制。
- **证据**：

```rust
// manifest.rs:515 —— 无任何上限，整文件进内存后再全文解析
let text = std::fs::read_to_string(&path).map_err(|e| ManifestError::Io { .. })?;
Self::from_yaml(&text, &path)     // 内部再 serde_yaml::from_str::<Value> 展开别名
```

```rust
// registry.rs:399 —— 模板文件同样无上限
let source_text = std::fs::read_to_string(&path).map_err(|e| { ... })?;
```

- **影响**：`templates.yaml` 是随项目分发的普通文件。一份 60 KB 的"锚点套锚点"清单（billion laughs 变体）可展开出 GB 级节点树，进程 OOM/被打死。打开他人仓库、或通过 HTTP 上传模板目录即触发。这是**唯一一条不需要认证就能让 CLI/HTTP 进程倒下的路径**。
- **改进建议**：
  1. 抽一个共用原语，三条路径统一走它（把 `preset.rs:222-239` 的 `Take` 逻辑上提到 `core/src/asset/mod.rs` 或新建 `core/src/io_limit.rs`）：
     ```rust
     pub fn read_text_capped(path: &Path, max: u64) -> Result<String, std::io::Error> {
         use std::io::Read;
         let mut text = String::new();
         std::io::BufReader::new(std::fs::File::open(path)?)
             .take(max + 1).read_to_string(&mut text)?;
         if text.len() as u64 > max { return Err(too_large(max)); }
         Ok(text)
     }
     ```
  2. 清单/模板建议上限 **8 MiB**（模板可含大段注释，1 MiB 偏小），机床配置沿用 1 MiB。
  3. 若必须支持超大清单，另行引入 `serde_yaml` 的 `-->` `saphyr` 或在解析前禁用别名（正则预扫 `&`/`*`）。
- **严重程度**：**P1**

---

### 【P1-2】`normalize()` 对"尚不存在的文件"不做父目录 canonicalize → "预设不得落模板根"红线可被绕过

- **位置**：`core/src/asset/preset.rs:649-676`（`normalize`）、`preset.rs:628`（`starts_with` 比较）
- **问题**：文件不存在时（"保存前校验"正是常态），`canonicalize(&abs)` 失败，回退分支里对 `out` 再 canonicalize **同样必然失败**（`out` 就是 `abs` 消掉 `..` 后的路径，末尾仍是那个不存在的文件），于是走 `_ => out`，拿到的**完全没有做大小写/8.3/符号链接归一**；而模板根通常存在、被 canonicalize 过。两侧"分辨率"不同，`starts_with` 恒假。
- **证据**：

```rust
// preset.rs:658-674
let resolved = std::fs::canonicalize(&abs).unwrap_or_else(|_| {
    ...
    match (std::fs::canonicalize(&out), out.file_name()) {
        (Ok(base), Some(name)) => base.join(name),   // ← 见 P2-3：这条分支还是错的
        _ => out,                                    // ← 实际永远走这里：未归一
    }
});
```

- **影响**：
  - **符号链接根**：`templates` 是 junction/symlink → 根被解析成 `D:\real\tpl`，预设侧仍是 `C:\proj\templates\presets.yaml` → 红线不触发 → 预设真被写进模板根。
  - **大小写差异**（Windows）：`--file TEMPLATES/presets.yaml` 对 `templates` 根，Rust 的 `Path::starts_with` 按 OsStr 字节比较、大小写敏感 → 恒假 → 绕过。
  - 后果：预设文件进入模板目录，被注册表目录扫描当作模板资产处理（R-9 红线本意正是防这个），且用户会发现"我明明指定了别处，文件却出现在 templates 下"。
- **改进建议**：改为**只对父目录 canonicalize**，文件名原样保留：
  ```rust
  fn normalize(p: &Path) -> PathBuf {
      let abs = /* 同现状 */;
      match std::fs::canonicalize(&abs) {
          Ok(c) => strip_verbatim(c),
          Err(_) => {
              let (parent, name) = match (abs.parent(), abs.file_name()) {
                  (Some(pa), Some(n)) => (pa, n),
                  _ => return strip_verbatim(abs),
              };
              let base = std::fs::canonicalize(parent)
                  .unwrap_or_else(|_| /* 逐段消 .. 的现有逻辑 */ resolve_dots(parent));
              strip_verbatim(base.join(name))
          }
      }
  }
  ```
  并把 `ensure_outside_template_root` 的比较改为**大小写不敏感**（Windows）/ 或统一 `to_ascii_lowercase()` 后再 `starts_with`。补一条用例：根是 symlink、预设路径在 symlink 之下 → 必须拒绝。
- **严重程度**：**P1**

---

### 【P1-3】`manifest_entry_body` 的字节切片可能在多字节空白缩进下 panic

- **位置**：`core/src/asset/template.rs:326`
- **问题**：`body.push(l[key_indent..].to_string())`。代码注释（`template.rs:324-325`）假设"首 `key_indent` 字节全为空白 → 切片落在合法 char 边界"。但 `indent_of` 用的是 `trim_start()`，它按 `char::is_whitespace` 裁剪，**包含 U+00A0（NBSP，2 字节）、U+2028 等多字节空白**。当键行缩进与字段行缩进的前缀构成不同时，字节下标会落进多字节字符内部。
- **证据**：

```rust
// template.rs:331-334
fn indent_of(line: &str) -> usize { line.len() - line.trim_start().len() }
// template.rs:321-327
if l.is_empty() || indent_of(l) <= key_indent { break; }
body.push(l[key_indent..].to_string());     // ← 可 panic
```

可复现输入（键行缩进 2 字节 NBSP、字段行前缀 `[空格, 空格, NBSP]`）：
```
templates:
  "\u{a0}a.j2":          # key_indent = 2
  \u{a0}name: "A"        # indent_of = 5 > 2 → &l[2..] 落在 NBSP 中间 → panic
```
- **影响**：`manifest_entry_body` 是 **pub** 并经 `lib.rs:39-42` 再导出的公共 API，且 `TemplateWriter::derive` 会用它。`derive` 一个含 NBSP 缩进的清单 → **进程 panic**（库里 panic 无法被调用方 catch）。虽是畸形 YAML，但清单是用户可编辑文件，"畸形输入 → 崩溃"不可接受。
- **改进建议**：换成不会 panic 的取法，并顺手把"缩进"定义改成只认 ASCII 空白：
  ```rust
  fn indent_of(line: &str) -> usize {
      line.bytes().take_while(|b| *b == b' ' || *b == b'\t').count()
  }
  // ...
  let Some(rest) = l.get(key_indent..) else { break };
  body.push(rest.to_string());
  ```
  补一条回归用例：NBSP 缩进不得 panic。
- **严重程度**：**P1**

---

### 【P1-4】`TemplateRegistry::new()` 内部 `.expect()` → 库构造路径 panic

- **位置**：`core/src/registry.rs:733`
- **问题**：`install_builtins()` 用 `expect("内置模板注册失败")` 注册内置模板；而 `new()` 是 `Default`、`GCodeGenerator::new()`、CLI 与 HTTP 启动路径的共同入口。任何一次内置模板编译失败（典型触发：minijinja/nctool-tpl 小版本升级后语法或过滤器语义变化）都会让进程在**启动瞬间** panic，且 panic 信息不含是哪个模板、哪一行。
- **证据**：

```rust
// registry.rs:732-733
// 内置模板注册失败视为编程错误（源码应为合法模板）
self.add_entry(entry).expect("内置模板注册失败");
```

- **影响**：崩溃 + 无诊断。CLI 侧 `context.rs:148/179/189` 每条命令、每个 HTTP 请求都要构造 `GCodeGenerator`/`TemplateRegistry`，此 panic 等于"任何命令都可能崩"。一个静态库应把"启动失败"表达成可诊断的错误，而不是 abort。
- **改进建议**（定稿：**降级方案，不改 `new()` 签名**）：
  ```rust
  // 不改 new()/Default 签名：内置模板注册失败 → 跳过该模板并留痕
  self.add_entry(entry).unwrap_or_else(|e| {
      let msg = format!("内置模板 {name} 注册失败，已跳过：{e}");
      eprintln!("nctool: {msg}");      // ① 库侧直接告警：不依赖任何调用方配合
      self.builtin_warnings.push(msg); // ② 结构化留痕：供 CLI / HTTP 查询
  });
  // 新增只读访问器
  pub fn builtin_warnings(&self) -> &[String] { &self.builtin_warnings }
  pub fn has_builtin_warnings(&self) -> bool { !self.builtin_warnings.is_empty() }
  ```
  **为什么不是"改 `new()` 返回 `Result`"**：`new()` 一旦返回 `Result`，`impl Default for TemplateRegistry`（`registry.rs:738-742`）就保不住（`Default` 必须返回 `Self`），而 `Default` 被 `GCodeGenerator::default()`（`pipeline.rs:373-377`）等依赖，冲击面比 CLI 调用点更大。降级方案对 CLI **零改动**。
- **⚠️ 降级方向对本项目更危险，必须配三重"响亮化"**（CLI reviewer 指出，已采纳）：崩溃会立刻被发现，而"某个内置模板悄悄不存在"不会——`templates list` 只是少一项，`render drill_cycle` 只报 `template_not_found`(5)。**对 CNC 工具，"静默产出错误程序"比 crash 更糟**，这是本项目自己定义的头号红线。故：
  1. **core 侧 `eprintln!`**（上例 ①）：保证即使调用方完全不接 `builtin_warnings()`，终端也至少有一次可见输出；
  2. **CLI 侧每进程一次 stderr**（`Ctx` 内 `std::sync::Once` 标记，HTTP 每请求构建 registry 不能每请求打印）+ `/health` 新增 `builtinWarnings` 字段（纯新增、不破坏契约，GUI 可据此提示）——由 CLI 侧实施；
  3. **守卫测试**：① 对 `builtin_templates()` 每个模板单独 `nctool_tpl::parse` 断言成功；② `builtin_registry_has_no_warnings()` 断言 `TemplateRegistry::new().has_builtin_warnings() == false`。把"minijinja/nctool-tpl 升级踩雷"从"用户运行时 panic"提前到"CI 变红"。
- **附带结论（回应 CLI 侧提问）**：用户模板 `{% include %}` 一个**缺失的内置模板**时，是**明确的硬失败**而非静默空内容，三处证据：
  1. `Renderer` 的 loader 是纯内存（`src/renderer.rs:184-186` `add_template_owned`），且 `core/src` / `cli/src` **从未调用** `set_path_loader`（全仓库 grep 0 处）→ 未注册模板名必然 `TemplateNotFound`；
  2. `with_lenient()`（`src/renderer.rs:126-129`）只设 `UndefinedBehavior::Lenient`（未定义**变量**渲染为空），**不改变**模板加载行为 → 宽松模式下 include 缺失模板同样硬失败；
  3. 全仓库模板**无一处**使用 `{% include ... ignore missing %}`（grep 0 命中）——这是唯一能让它静默变空的写法。
- **严重程度**：**P1**

---

### 【P1-5】`rename` 后未 fsync 父目录 → 崩溃/掉电后原子写不具持久性

- **位置**：`core/src/asset/atomic.rs:104-108`（`write_atomic_once`）
- **问题**：`write_tmp` 做了 `flush` + `sync_all`（`atomic.rs:118-120`），数据本身落盘了；但 `rename` 之后**没有对父目录 `sync_all`**。POSIX 语义下，目录项（directory entry）的更新只进了页缓存，进程崩溃或掉电后 rename 可能整体丢失，表现为"旧文件还在 / 新文件不存在"。
- **证据**：

```rust
// atomic.rs:104-108
if let Err(err) = std::fs::rename(&tmp, path) {
    let _ = std::fs::remove_file(&tmp);
    return Err(map_io(err, path));
}
Ok(())          // ← 父目录从未 sync
```

- **影响**：`write_atomic` 的文档（`atomic.rs:5-8`）承诺"目标任何时刻要么是旧内容、要么是新内容"。在**进程可见性**层面这是对的，但在**持久性**层面不成立。机床参数文件写到一半断电，用户可能拿到"修改前"的旧参数而不自知——对 CNC 场景是危险方向。
- **改进建议**：rename 成功后补一次父目录 fsync（Windows 上同样有效且开销可忽略）：
  ```rust
  std::fs::rename(&tmp, path).map_err(...)?;
  // 目录项本身也要落盘：否则掉电后 rename 可能整体丢失
  if let Ok(dir) = std::fs::File::open(parent) {
      let _ = dir.sync_all();   // 失败不阻断写入已完成，仅记录
  }
  Ok(())
  ```
  并在 `atomic.rs` 文档里把"原子"精确表述为"原子可见 + 尽力持久"。
- **严重程度**：**P1**

---

### 【P1-6】`SafePath::resolve` 的 `exists()` → `canonicalize()` 存在 TOCTOU，且返回的是未归一路径

- **位置**：`core/src/asset/path.rs:87-100`
- **问题**：先 `candidate.exists()` 判断、再 `canonicalize()`；两步之间目标可被替换（例如原本不存在的位置被放入指向根外的 symlink，或原本是普通文件被换成 symlink）。更关键的是：**校验用的是 `checked`，返回给调用方的是 `candidate`**（`path.rs:100`），调用方拿到的是未经符号链接解析的路径，随后的 `File::create` / `rename` 作用在 `candidate` 上。
- **证据**：

```rust
// path.rs:87-100
let checked = if candidate.exists() { candidate.canonicalize()? } else { candidate.clone() };
if !checked.starts_with(&self.root) { return Err(WriteError::PathEscape { .. }); }
Ok(candidate)      // ← 返回的是未 canonicalize 的路径
```

- **影响**：
  1. 时间窗内替换成 symlink → 第一次写入会**跟随符号链接**写到根外（`File::create` 跟随链接）。虽然后续 `rename` 会替换目录项本身、不再跟随，但"第一次创建"这一下已经写穿了。
  2. 在 Windows 上若 `candidate` 是目录 symlink / junction，`File::create` 会失败或写入 junction 目标。
- **改进建议**：
  - 返回 canonicalize 后的路径给调用方（对不存在的目标，canonicalize 父目录后 join 文件名），使"校验对象"与"使用对象"是同一条路径；
  - 打开目标时用 `OpenOptions::new().write(true).create(true).custom_flags(O_NOFOLLOW)`（Unix）/ `FILE_FLAG_OPEN_REPARSE_POINT`（Windows）拒绝跟随链接；
  - 至少把 `exists()` 换成 `symlink_metadata()`，并对"是符号链接"直接报 `PathEscape`（创建资产本来就不该落到链接上）。
- **严重程度**：**P1**

---

### 【P1-7】`TemplateWriter::create` / `derive` 的重名检查非原子（文档自认，但未修）

- **位置**：`core/src/asset/template.rs:100-113`、`template.rs:136-147`
- **问题**：`create` 用 `write_guarded(expect=None)` 表达"文件必须不存在"，实现是 `read_snapshot` 再写，属 check-then-write。
- **证据**：

```rust
// template.rs:100-104（注释原文）
/// ⚠️ **这不是"原子创建"**：... 两个并发 `new` 同名可以
/// **双双通过检查**，后 `rename` 者胜、**静默覆盖**。
```
- **影响**：两个进程同时 `template new` 同名 → 一个模板被静默覆盖，无 Conflict、无日志。
- **改进建议**：**由 P0-1 的锁一并解决，不新增 API。** `write_guarded` 全程持锁后，「读快照 → 比对 → 写」期间不可能被另一个 cooperating writer 插队，`expect = None` 的检查即为真原子。`TemplateWriter::create` / `derive` **签名与返回的错误变体均保持现状**（仍返回 `WriteError::Conflict`）。
- **为什么不用 `O_EXCL`（原建议已撤回）**：`create_new` 必须**直接写目标文件**（tmp + rename 那套用不上 `O_EXCL`——rename 的目标是否存在与 tmp 无关），而 `atomic.rs:6-8` 明确写着"直接 `open + truncate + write` 覆盖目标，写到一半时崩溃会留下**半成品**"。`O_EXCL` 虽避免覆盖旧文件，但崩在半路会留下**残缺的新文件**，正是 tmp+rename 要避免的形态——堵了并发却引入崩溃残留，是反向交易。此外改为 `O_EXCL` 会让重名错误退化成 `Io`，破坏 `template_duplicate`(6) / `name_conflict`(6) 的对外退出码契约。
- **严重程度**：**P1**

---

## 三、P2-一般

### 【P2-1】写路径重复整文件读取（预设写一次读 3~4 遍）

- **位置**：`core/src/asset/preset.rs:313-314`、`340+347`、`365+372`、`core/src/asset/mod.rs:89`
- **证据**：`upsert` 先 `Self::load(path)?`（整读 + 全文下溢扫描 + YAML 解析），再 `WriteKernel::read_fingerprint(path)?`（**再整读一遍**只为算指纹），最后 `save` → `write_guarded` → `read_snapshot`（**第三次整读**）+ 写后 `read_fingerprint`（**第四次**）。`machine.rs:129+173`、`template.rs:253` 同构。
- **影响**：`presets.yaml` 每写一次发生 4 次全量 IO + 1 次全文下溢扫描；在 HTTP 批量导入场景下放大明显。同时重复读取拉长了 P0-1 的 TOCTOU 窗口。
- **建议**：让 `read_snapshot` 一次返回 `(bytes, fingerprint)`，上层复用：`PresetStore::load` 增加 `load_with_fingerprint()` 返回 `LoadOutcome { file, fingerprint, .. }`，`upsert/rename/remove` 直接用；`write_guarded` 写后的指纹由已写入的 `bytes` 直接计算（`FileFingerprint::of_bytes(bytes, mtime)`），不再回读。

### 【P2-2】`TemplateManifest::from_yaml` 对同一文本做两次完整 YAML 解析

- **位置**：`core/src/manifest.rs:440-441` 与 `manifest.rs:461-462`
- **证据**：先 `serde_yaml::from_str::<Value>(text)` 判有无 `templates` 键，命中后再 `serde_yaml::from_str::<ManifestFile>(text)` 完整重解析。外加 `manifest.rs:420` 的全文下溢扫描，共 3 趟。
- **建议**：改成"解析一次到 `Value`，然后从 `Value` 反序列化"：`serde_yaml::from_value::<ManifestFile>(value)`（`serde_yaml` 提供该 API），一趟搞定，还能保留 `deny_unknown_fields`。

### 【P2-3】`normalize()` 的 `(Ok(base), Some(name)) => base.join(name)` 分支会拼出重复文件名

- **位置**：`core/src/asset/preset.rs:671`
- **证据**：`base` = `canonicalize(&out)` 的结果（**已含文件名**），再 `join(name)` → `…/presets.yaml/presets.yaml`。
- **影响**：该分支当前实际不可达（见 P1-2），但一旦有人"修好"了父目录 canonicalize 就会立刻产出错误路径，把红线判成"在根外"。
- **建议**：随 P1-2 一并重写为 `canonicalize(parent).join(name)`。

### 【P2-4】`WriteKernel::read_fingerprint(...).ok().flatten()` 吞掉错误

- **位置**：`core/src/asset/template.rs:253`
- **证据**：`let expect = WriteKernel::read_fingerprint(path).ok().flatten();` —— 读取失败被折叠成"没有期望"。
- **影响**：若 `metadata` 失败（权限/竞态），`expect` 变成 `None`，语义从"我接受当前内容"变成"文件必须不存在"，把一次 IO 故障伪装成一次冲突，错误信息丢失。
- **建议**：`let expect = WriteKernel::read_fingerprint(path)?;` 并把错误归到 `ManifestOutcome::Degraded(format!("清单指纹读取失败：{e}"))`。

### 【P2-5】`read_snapshot` 的 `modified().unwrap_or(UNIX_EPOCH)` 静默降级

- **位置**：`core/src/asset/guard.rs:82`
- **影响**：文件系统不支持 mtime 时，三元组退化成二元组；注释说"不影响正确性"，但会让"同内容不同时间"这类覆盖检测漏掉。
- **建议**：保留降级但记一条 `debug_assert` 或 trace 日志，避免将来排障时无从察觉。

### 【P2-6】`WriteError::Conflict` 的 Display 不含指纹值

- **位置**：`core/src/asset/mod.rs:202-204`
- **证据**：`write!(f, "写入冲突：{} 已被外部修改（乐观锁失败）", path.display())` —— `expected` / `actual` 两个字段一个都没输出。
- **影响**：CLI `--expect-hash` 排障时用户看不到"期望什么、实际是什么"。
- **建议**：补上 `expected: {}/actual: {}`（用 `FileFingerprint::as_string()`，`None` 显示为 `<不存在>`）。

### 【P2-7】临时文件无启动清扫，异常退出会永久残留

- **位置**：`core/src/asset/atomic.rs:98-108`
- **证据**：tmp 名为 `<目标>.nctool-tmp-<pid>-<nanos>.tmp`；只有在 `write_tmp` 或 `rename` **返回 Err** 时才 `remove_file`。若进程在两者之间被杀（Ctrl-C / OOM / 断电），tmp 永久留在模板根。
- **影响**：模板根里出现 `demo.j2.nctool-tmp-1234-….tmp` 垃圾文件。虽然 `.tmp` 后缀不会被 `*.j2` 扫描收录（设计已考虑），但会污染 `git status` 与目录列表；长期累积。
- **建议**：在 `TemplateRegistry` 扫描模板目录或 `SafePath::from_root` 时做一次 best-effort 清扫：删除匹配 `*{TMP_INFIX}*{TMP_SUFFIX}` 且 pid 已不存在的条目。至少在 CLI 加一个 `nctool doctor --clean-tmp`。

### 【P2-8】`TemplateWriter::rename` 完全没有乐观锁

- **位置**：`core/src/asset/template.rs:153-163`
- **证据**：`rename(root, old_rel, new_rel)` 签名里没有 `expect` 参数；内部 `std::fs::read(&old_path)` 后直接写。
- **影响**：与 `save`（有 `expect`）不对称；重命名期间源模板被别人改过也无从察觉。
- **建议**：加 `expect: Option<FileFingerprint>` 参数（对旧文件取快照），与 `save` 同口径。

### 【P2-9】`build_derived_source` 把 CRLF 源统一成 LF

- **位置**：`core/src/asset/template.rs:439`（`out.push_str(&lines.join("\n"))`）
- **影响**：`str::lines()` 会剥掉 `\r`，Windows 上派生的 CRLF 模板产出 LF 文件，全仓库换行符不一致；与本模块 `line_sep()`（`template.rs:387-393`）显式保全 CRLF 的口径自相矛盾。
- **建议**：用 `line_sep(src)` 作为 join 分隔符，并按 `src` 是否含 `\r\n` 决定是否保留。

### 【P2-10】`MachineWriter::load` 的键与 `MachineConfig.id` 可能不一致

- **位置**：`core/src/asset/machine.rs:420-458`
- **证据**：`cfg.id` 初始为段名 `id`，但 `machine.rs:426-428` 会用表内 `id = "other"` 覆盖；`machine.rs:458` 仍以**段名**为键插入 map。
- **影响**：`load()["a"].id == "b"` 这类自相矛盾的结果，下游按 id 反查时会错。
- **建议**：要么以 `cfg.id` 为键（`out.insert(cfg.id.clone(), cfg)`），要么忽略表内 `id` 强制等于段名；并在 `upsert` 前置校验两者一致。

### 【P2-11】`MachineWriter::preflight` 在循环内线性查 `KNOWN_CONFIG_KEYS`

- **位置**：`core/src/asset/machine.rs:265-268`
- **影响**：O(键数 × schema 数)。当前量级很小，但 `preflight` 在每次 `machine add/edit` 都跑，且 schema 会随机床类型增长。
- **建议**：函数开头一次性建 `HashMap<&str, &MachineKeySchema>`：`let schema: HashMap<_, _> = KNOWN_CONFIG_KEYS.iter().map(|s| (s.key, s)).collect();`

### 【P2-12】`manifest` 与 `registry` 双向依赖

- **位置**：`core/src/manifest.rs:27`（`use crate::registry::TemplateCategory`）、`core/src/registry.rs:154`（`pub status: Option<crate::manifest::TemplateStatus>`）
- **影响**：`manifest ⟷ registry` 在同一 crate 内形成环。Rust 允许（同一编译单元），但职责边界模糊：分类枚举属于"注册表"、状态枚举属于"清单"，两边互相引用意味着**任何一个都不能独立演进/拆 crate**。
- **建议**：把 `TemplateCategory` 与 `TemplateStatus` 下沉到 `model.rs`（数据模型层），`manifest` 与 `registry` 都只依赖 `model`。这是最小改动、收益明确的一次解环。

### 【P2-13】`TemplateEntry.source_text` 是 pub 可变字段，而 `analysis` 是私有缓存

- **位置**：`core/src/registry.rs:136-159`（字段）、`registry.rs:227-240`（`analysis()` / `invalidate_analysis()`）
- **问题**：`source_text: String` 是 `pub`，外部可随意改；`analysis: OnceCell<…>` 是私有且带缓存。文档（`registry.rs:224-226`）只能靠"调用方记得调 `invalidate_analysis()`"来维持一致性。
- **影响**：典型的**抽象泄漏 + 不变式靠约定**。漏调一次 → `extract_params`/`validate` 基于旧源码的变量表校验新源码，结论全错且不报错。
- **建议**：`source_text` 改私有，提供 `pub fn set_source_text(&mut self, s: String)`，内部顺手 `self.analysis = OnceCell::new();`。破坏性变更可接受（crate 未 1.0），或用 `#[deprecated]` 过渡。

### 【P2-14】`TemplateRegistry` 用 `std::cell::OnceCell` → 类型 `!Sync`

- **位置**：`core/src/registry.rs:159`、`registry.rs:330`
- **影响**：`TemplateRegistry` 无法 `Arc` 共享给多线程（HTTP 服务必须整个 `Mutex<Registry>`），所有请求串行化。
- **建议**：两个 `OnceCell` 换成 `std::sync::OnceLock`（API 几乎一致），`TemplateRegistry` 立刻变 `Sync`，HTTP 侧可用 `RwLock` 或只读共享。

### 【P2-15】`pipeline` 的默认值兜底未使用 include 闭包规格

- **位置**：`core/src/pipeline.rs:258`（`apply_spec_defaults(&entry.params, &derived)`）vs `registry.rs:557-559`（`specs` 已并入 include 闭包）
- **影响**：校验按闭包规格判"有默认值即算已提供"，渲染时却只按 `entry.params` 注入默认值 → **被 include 子模板声明的默认值不会注入**，严格模式尚可（会报缺失），宽松模式会渲染出空值。两处口径不一致。
- **建议**：让 `registry` 暴露 `specs_closure(name)`（复用 `collect_include_closure`），`pipeline` 用它做兜底；或在 `validate` 的返回值里带上闭包规格。

### 【P2-16】`postprocess` 的小写前缀识别只对默认 `O`/`N` 生效

- **位置**：`core/src/pipeline.rs:524-527`
- **证据**：`(program_prefix == "O" && trimmed.starts_with('o'))`、`(line_prefix == "N" && trimmed.starts_with('n'))` —— 用户自定义 `program_prefix = "P"` 时，小写 `p1000` 不会被识别为程序号行，会被重复编号。
- **建议**：改成对配置值本身做 ASCII 小写比较：`trimmed.to_ascii_lowercase().starts_with(&program_prefix.to_ascii_lowercase())`。

### 【P2-17】`PresetStore::import_presets` 缺少重名与 `template` 字段校验

- **位置**：`core/src/asset/preset.rs:416-424`
- **证据**：只校验了 `name`（`validate_asset_name`）与注入的 `validate`；文件内**同名预设互不查重**，`p.template`（可含 `../`）无校验，导入文本也无字节上限。
- **建议**：导入后 `BTreeSet` 查重（`deny_unknown_fields` 挡不住 YAML 重复键已由 `manifest_is_parseable` 那套思路覆盖），并对 `template` 走 `validate_asset_name` 的分段校验；HTTP 侧务必在调用前限制 body 大小（core 侧可加 `MAX_IMPORT_BYTES`）。

### 【P2-18】`PresetStore::save` 只校验文件名，且会 `create_dir_all(parent)`

- **位置**：`core/src/asset/preset.rs:283-296`
- **影响**：路径其余部分完全信任调用方；写操作会**凭空创建目录树**。若上层（HTTP）未做路径约束，等价于任意目录创建。
- **建议**：core 侧增加一个可选的"允许的根"参数，或至少在文档里把"调用方必须先过 `ensure_outside_template_root` 与白名单"写成契约；CLI/HTTP 统一调用。

### 【P2-19】`strip_verbatim` 用 `to_string_lossy()` 重建路径

- **位置**：`core/src/asset/preset.rs:680-688`
- **影响**：非 UTF-8 路径（Windows 上常见）被 lossy 替换成 `U+FFFD`，后续比较/写入都指向错误路径，且不报错。
- **建议**：改用 `Path::components()` / `as_os_str()` 逐段处理，避免 `to_string_lossy`。

### 【P2-20】清单键未拒绝 `..` / 绝对路径 / 盘符

- **位置**：`core/src/manifest.rs:591-597`（`normalize_key` 只做 trim、`\`→`/`、去 `./`）
- **影响**：`"../../etc/x.j2"`、`"C:/Windows/x.j2"` 会作为合法键进入 `entries`。当前只用作 map 键、不拼进 fs 路径（风险未兑现），但 `orphan_keys` / `machine_packs` / UI 展示会把这些字符串原样吐出，且一旦将来有人拿键去拼路径就是现成的穿越向量。
- **建议**：`from_entries` 里对归一化后的键复用 `validate_asset_name` 的**分段**校验（复用 `template.rs:176` 的 `resolve_rel` 思路），非法键直接 `ManifestError::Parse`。

### 【P2-21】`extract_params_block` 的 `trim_matches('-')` 过度裁剪

- **位置**：`core/src/manifest.rs:729`
- **证据**：`let content = inner.trim().trim_matches('-').trim();` —— 会把行首尾**所有** `-` 去掉。
- **影响**：以 `-` 开头/结尾的合法参数名（如 `-Z`、`X-`）被静默改形。
- **建议**：只剥 minijinja 的空白控制符，即 `trim_end_matches('-')` 限一次：`inner.trim_end().trim_end_matches(|c| c == '-' || c.is_whitespace()).trim()`。

### 【P2-22】公共 API 表面积过大 + 双路径导出

- **位置**：`core/src/lib.rs:31-72`（约 40 个 re-export）、`core/src/asset/mod.rs:28-42`（再 `pub use` 一遍）
- **影响**：同一符号有 `nctool_core::Preset` 与 `nctool_core::asset::Preset` 两条路径；`lib.rs` 一次性导出 `asset` / `model` / `validate` / `registry` / `manifest` / `pipeline` / `part` / `machine` / `json_num` 的全部主力类型，API 冻结成本高。
- **建议**：`lib.rs` 只导出"面向消费方的一层"（`Pipeline / Registry / Write 内核 / 错误类型`），底层模块保留 `pub mod` 由需要者自行 `use`；删除 `asset/mod.rs` 中与 `lib.rs` 重复的 `pub use` 组。

### 【P2-23】四套错误类型无统一 `kind()` 分类

- **位置**：`core/src/asset/mod.rs:143`（`WriteError`）、`core/src/registry.rs:271`（`RegistryError`）、`core/src/manifest.rs:960`（`ManifestError`）、`core/src/pipeline.rs:16`（`PipelineError`）
- **影响**：四个枚举都 `#[non_exhaustive]`，HTTP 层要为每一套写一遍"变体 → 状态码"的 match。新增变体时四处都要记得补，漏一处就是把 4xx 报成 500（文档 `mod.rs:169-174` 已记录过一次这类事故）。
- **建议**：给四个枚举各加一个 `pub fn kind(&self) -> ErrorKind`（`NotFound / InvalidInput / Conflict / Permission / Internal / Corrupt`），HTTP 只 match `kind()`；新增变体时编译器通过穷尽匹配强制表态。

### 【P2-24】校验阶段静默跳过未注册的 `include` 引用 → 参数表"延迟发现"

- **位置**：`core/src/registry.rs:589-591`（`collect_include_closure` 内 `let Some(sub) = self.entries.get(ref_name) else { continue; }`）、`registry.rs:582-584`（`let Ok(analysis) = entry.analysis() else { return; }`）
- **证据**：
```rust
// registry.rs:589-592
let Some(sub) = self.entries.get(ref_name) else {
    continue; // 引用未注册模板：渲染期报 TemplateNotFound，此处无法静态并入
};
```
- **影响**：`validate` / `extract_params` 对未注册（或解析失败）的被引用模板**既不报错、也不并入其参数**。于是 `inspect` 列出的参数表**少列**该子模板的参数，用户按表填参后直到渲染才失败。属"延迟发现"而非"静默产出错误程序"（渲染会硬失败，见 P1-4 附带结论），但会让 `inspect` 的参数表不可信——用户会认为"表上这些就够了"。
- **建议**：`extract_params` 增加"未解析 / 未注册的引用"清单字段（如 `pub struct ExtractedParams { vars: Vec<Variable>, unresolved_refs: Vec<String> }`），由 CLI 侧作为 warning 呈现，把延迟发现提前到 `inspect` 阶段。

---

## 四、优先修复的 TOP 3

| # | 问题 | 位置 | 严重程度 | 理由 |
|---|------|------|---------|------|
| **1** | 乐观锁 check-then-write，全 crate 零互斥 → 并发写静默互相覆盖 | `asset/mod.rs:72-88` | **P0** | 与模块自述的设计承诺直接冲突；丢失的是直接进 G-code 的机床/预设参数；HTTP 与 CLI 并存场景下命中概率不低。修法明确：同目录 `.nctool.lock` + OS advisory lock + RAII，配套 `WriteError::LockBusy`（→ CLI `write_conflict`，退出码 6）。P1-7 由本项一并解决。 |
| **2** | 清单/模板/机床配置三处无读取上限 + serde_yaml 无别名预算 → 内存放大 DoS | `manifest.rs:515`、`registry.rs:399`、`machine.rs:335` | **P1** | 唯一一条无需认证即可让进程倒下的路径，且 `presets.yaml` 已有上限、其它三条没有，**同口径不一致本身就是缺陷**。修法是把 `preset.rs:222-239` 的 `Take` 逻辑上提共用。 |
| **3** | `normalize()` 对不存在文件不做父目录 canonicalize → "预设不得落模板根"红线可被 symlink / 大小写差异绕过 | `asset/preset.rs:649-676`、`628` | **P1** | 这是项目明写的红线（R-9），而"保存前校验"恰恰就是文件不存在的常态场景 → 红线在最该生效的时刻失效。修法小（canonicalize 父目录 + join 文件名）且能顺带修掉 P2-3 的错误分支。 |

> 紧随其后应处理的两条：**P1-3**（`manifest_entry_body` 可 panic，公共 API 崩溃）与 **P1-4**（`TemplateRegistry::new()` 内 `.expect()`），两者都是"一处小改动即可消除的崩溃路径"，性价比极高。

---

## 五、架构层面整体评价

1. **写通道的抽象是本项目最漂亮的一笔**：把"原子写 + 乐观锁 + 路径防护 + 单一入口"收敛到 `asset::WriteKernel` 一处，并配套 `toml_edit`（保注释）/ 定点文本编辑（保字节）/ serde 往返（自有文件）三种差异化的编辑策略——这是真正理解"不同文件有不同的不可丢失属性"之后才做得出的分层，值得保留并在其它模块推广。
2. **主要架构债是"声称的保障与实际的机制不匹配"**：`guard.rs` 名为锁实为指纹、`write_guarded` 名为锁实为 check-then-write、`create` 注释自认非原子、`source_text` 的缓存不变式靠调用方自觉——**每一处都写了详尽的文档解释为什么，却都把"真正落地机制"留给了未来**。文档质量掩盖了机制缺失，这比没有文档更危险，因为它会让 review 者误以为已经解决了。
3. **模块边界存在两处需要动刀的耦合**：`manifest ⟷ registry` 双向依赖（应把 `TemplateCategory`/`TemplateStatus` 下沉到 `model`），以及 `asset::machine` 反向依赖 `pipeline::MAX_LINE_NUMBER_DIGITS`（写层依赖渲染层常量，虽然注释解释了是为了"单一来源不复制夹紧逻辑"，但方向仍是反的——该常量应放在 `model` 或独立 `limits` 模块）。
4. **错误类型设计有清晰的自觉**（`NotFound` 独立于 `Corrupt` 以避免"靠消息文本分类"、`NumUnderflow` 独立于 `Corrupt` 以避免降级策略误吞硬失败、`WriteAction::Deleted` 独立于 `Updated`），但**四套枚举彼此孤立**，缺少统一的 `kind()` 让消费方可以结构化分支；这是当前 HTTP 层最容易在新增变体时静默退化的地方。
5. **输入校验的"深度"做得好、"广度"有缺口**：数值下溢硬失败（ERR-NUM-UNDERFLOW，三通道同口径）、`deny_unknown_fields` 全面启用、FNV-1a 规范化串用长度前缀防注入——这些都超出同类项目水平；但**"读取上限"只在一条通道上做了，其余三条裸奔**，形成了明显的能力洼地（P1-1）。把已验证的做法推广到全部输入通道，是投入产出比最高的一次架构收敛。

---

*报告完 · 高见远*
