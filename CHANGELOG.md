# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/) 格式和 [语义化版本](https://semver.org/)。

## 版本策略

- **0.x 阶段**：API 仍在演进，`minor` 版本升级可能包含破坏性变更（如本版本的 API 收窄、错误细分）。
- **1.0.0**：公共 API 稳定后发布，此后严格遵循语义化版本（`major` = 破坏性变更，`minor` = 向后兼容新功能，`patch` = 向后兼容修复）。
- 破坏性变更均在 `Changed` 节明确标注，并在 README 中说明迁移方式。

---

## [未发布]

本段收录 2026-09-18 第三轮代码审查（`docs/CODE_REVIEW_2026-09-18.md`）的修复，按报告 §3
的批次顺序分两组：

- **批次一（堵住静默出错）**：五条「不报错但产出错误数值」的路径。改动全在 `core`，
  未触及公共 API 形状（`DeriveError` 新增变体，该枚举本就 `#[non_exhaustive]`）。
- **批次二（修好度量闭环）**：覆盖率门禁口径、自证测试、MSRV 声明、CI 卫生。
  **不改变任何运行时行为**，只让既有的质量声明从「文档里的一句话」变成可验证的事实。

### 批次一：堵住静默出错

#### Fixed

- **派生参数的链式依赖不再取错源值**（`core/src/derive.rs`）：`A.derive.from = B`
  且 `B` 自身也是派生参数时，A 的查表键此前取自 `with_defaults`（用户值 / 规格默认值），
  而 `out`（本轮已算出的派生值）只写不读。于是「用户传了 B」用旧值查表、「B 有默认值」
  用默认值查表 —— 两条路径都不报错，直接写出与型号不符的 G-code。
  现改为按依赖顺序（重复扫描到不动点）计算，源参数若也是派生参数则取**已算出的派生值**；
  互相依赖成环时返回新增的 `DeriveError::Circular`，不静默取任何值。

- **条件必选不再被模板内联兜底短路**（`core/src/validate.rs`）：`check_missing` 里
  `has_default`（含 `var.optional`）的判定早于 `required_if` 判定，导致模板写
  `{{ FS_Z_PLUS1 | default(0) }}` 时，清单里声明的 `required_if` **永久失效且无任何提示**
  —— 互斥分支参数被 `default(0)` 兜底后静默产出 `Z0`，正是 `undercut_fs.j2` 头部注释
  与 `templates.yaml:47-51` 明令禁止的场景。现改为：声明了 `required_if` 的参数，
  其必选性不受 `var.optional` 影响（规格显式声明的 `default` 仍算兜底）。

- **有限性检查递归进入列表**（`core/src/validate.rs`）：`check_finite` 此前只匹配顶层
  `Number`，`passes=[{z: NaN}]` 会「校验通过、渲染失败」（渲染期 `ensure_finite_context`
  是递归的），错误类型从 `Validation` 变成 `Render`，宽松模式「唯一硬失败项 = NonFinite」
  的承诺随之失真。现递归列表元素，消息带元素下标路径（如 `[1][0]`），
  并沿用与渲染期一致的 32 层深度上限。

- **规格默认值做有限性检查**（`core/src/validate.rs`）：`check_spec_defaults` 此前只查
  类型 / 白名单 / 区间，`default: .nan` 会经类型检查、绕过区间比较（NaN 的任何比较都是
  false）、且白名单通常未声明 —— 校验全绿，NaN 却在渲染前被静默注入上下文。

- **空前缀不再让整份程序静默不编号**（`core/src/pipeline.rs`、`core/src/machine.rs`）：
  机床配置 `line_number_prefix = ""` / `program_prefix = ""` 会让 `starts_with("")` 恒真，
  每一行都被判为「程序号行」或「已有行号」，`line_numbers: true` 下**一行都不编号且无告警**。
  键缺失与键为空串现在同等回退默认值（新增 `non_empty_config`）；
  `validate_config_keys` 对字符串类键补「非空」校验。

#### 测试

- 新增 10 项回归测试（derive 3 / validate 4 / pipeline 2 / machine 1），
  均已**反向验证**：临时回滚实现后全部 FAILED，恢复后通过。
- workspace 全量 **536 项**通过；`cargo fmt --all --check`、
  `cargo clippy --workspace --all-targets -- -D warnings`、
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` 均通过。

### 批次二：修好度量闭环

起因是一类不同于「代码错」的问题：**度量在骗人**。三处质量声明在当前仓库里都不成立 ——
覆盖率门禁的数字与它声称的口径对不上，退出码矩阵的用例删掉矩阵也照样绿，MSRV 写在
三处 `Cargo.toml` 却没有任何 CI 任务在 1.82 上编译过。本批次不修任何生产逻辑。

#### Changed

- **覆盖率门禁改用生产口径**（`.github/workflows/ci.yml`、新增
  `scripts/check_coverage_caliber.py`）：`cargo llvm-cov` 把 `src/*.rs` 内的
  `#[cfg(test)]` 段本身计入分母（`src/lib.rs` 1824 行里测试段从第 64 行起占 1760 行），
  于是「新增测试」会**推高**覆盖率数字、「新增未覆盖的生产代码」反被稀释：门禁显示
  92.99%，剔除测试段后的生产代码只有 88.65% —— **门禁绿 ≠ 生产代码达标**。
  `--ignore-filename-regex` 只能按文件路径排除，管不到 `src/` 内部的测试段，
  故改由脚本从 lcov 数据剔除测试段后重新统计。
  阈值 `--fail-under-lines 90`（llvm-cov 口径）随之废弃，改为脚本口径的
  `--min 88`（当前基线 88.65% / 本次实测 88.76%，余量约 32 行）。

- **退出码矩阵用例不再自证**（`cli/tests/cli_e2e.rs`）：原用例断言的是函数内硬编码的
  局部数组 `[(0, "成功"), (1, "校验未通过"), ...]`，与 `CliError::exit_code` 无任何
  链接 —— 删掉矩阵里任一码、或改坏 `exit_code()` 的映射，它都照样绿。
  现改为读 `README.md` 的退出码表格，断言其**连续覆盖 `0..=7`**（无缺号、无多余）。
  守的是文档侧契约（README 那张表是脚本作者判分支的依据）；`kind → 退出码` 的映射
  由 `cli/src/output.rs` 的 `exit_code_matrix` 单元测试逐个钉住，不在此重复。

- **CI 卫生**（`.github/workflows/ci.yml`）：
  - `Test` 步骤加 `--locked`：`Cargo.lock` 已提交，CI 必须与它一致，否则
    「本地能过、CI 红了」无解。
  - 新增 `Doc tests` 步骤（`cargo test --workspace --doc`）：实测
    `--all-targets` **不跑 doctest**（输出里没有 `Doc-tests` 段），而 README 声称
    「单元 + 集成 + 文档」全跑。
  - `cargo-audit` 改用 `taiki-e/install-action@cargo-audit`：此前
    `cargo install cargo-audit --locked` 要在三个 runner 上各从源码编译一遍
    （连续两次卡在本步骤 exit 101），且未固定版本 —— 上游换新版即整条 CI 红。
  - `cargo audit` 加 `--deny warnings`：默认只报 vulnerability，
    unmaintained / yanked 不失败。

- **新增 MSRV job**（`.github/workflows/ci.yml`）：`rust-version = "1.82"` 写在三个
  `Cargo.toml` 里、README 与 CONTRIBUTING 也对外承诺 1.82+，但此前**没有任何任务在
  1.82 上编译过** —— 依赖任一 minor 抬高 MSRV 都会让这个承诺静默失真。
  该 job 只 `check` 且**不带 `--all-targets`**：`rust-version` 承诺的是「下游能否编译
  本 crate」，而下游构建的是 lib + bin；带上 `--all-targets` 会把 criterion /
  assert_cmd 等 dev-dependencies 拉进来，它们的 MSRV 通常高于生产依赖，
  属于本仓库自己的开发工具链问题，不该让对外承诺失真。

- **golden 刷新加 CI 守卫**（`core/tests/integration.rs`）：`NCTOOL_UPDATE_GOLDEN`
  命中即写文件并 `return`，跳过该函数的**全部**断言 —— 21 组 golden 会静默退化成
  「跑得通即通过」。当前 workflow 没有设这个变量，但 `env:`、`.cargo/config.toml`
  或某个 runner 的默认环境都可能把它带进来，一旦带进来没有任何东西会报错。
  现加前置守卫：`CI` 存在时直接 panic；本地刷新路径不受影响。

- **覆盖率相关表述全面对齐**（`README.md`、`docs/CONTRIBUTING.md`、`docs/RELEASE.md`、
  `docs/ROADMAP.md`、`docs/PROJECT_STATUS.md`）：此前文档与 CI 对「覆盖率是否阻断」
  自相矛盾 —— `CONTRIBUTING.md:28` 写「非阻断」而**同一文件** `:74` 写「阻断」，
  `README.md:589`、`RELEASE.md:37` 也写「非阻断」，而 `ci.yml` 早已移除
  `continue-on-error`。按文档行事的人会把红的 coverage 当噪音合并，等于从语义上把
  门禁又拆掉。现统一为「阻断」，连同上面那条口径变更一起改：
  - 本地复现命令换成 `cargo llvm-cov ... --lcov` + `check_coverage_caliber.py` 两行；
    「上调阈值改哪里」由 `ci.yml` 的 `--fail-under-lines` 改为脚本的 `--min`
  - `CONTRIBUTING.md` §4 测试矩阵**去掉硬编码项数**（同一份文档曾同时存在
    344 / 516 / 492 三个数字），指向 CI job summary；补上此前漏列的
    `tests/extract_invariant.rs`，并标注 `--all-targets` 不跑 doctest
  - `ROADMAP.md` §0 TL;DR 与 `PROJECT_STATUS.md` §0/§4/§8 更新到最新实测，
    并在两处历史记录旁标注「09-18 口径更正」，避免旧数字被当现状读
  - README 与 CONTRIBUTING 的本地质量门命令块补 `cargo test --workspace --doc`，
    与 CI 的单列步骤一致

#### 测试

- 覆盖率脚本对重跑后的 `lcov.info` 实测：生产口径 **88.65%**（4319/4872），门禁通过；
  同时打印 llvm-cov 原始口径 92.99%（9538 行中 6908 行位于 `#[cfg(test)]` 段）
  与被剔除的行数，便于核对口径差异。
- golden 守卫**反向验证**：`CI=1 NCTOOL_UPDATE_GOLDEN=1 cargo test -p nctool-core
  --test integration` → FAILED（panic 于守卫，且在写文件之前）；去掉 `CI` 后本地
  刷新路径仍可用，21 组基线字节未变（`git status tests/golden/` 干净）。
- `exit_code_matrix_in_docs_is_complete` 改为读 README 断言后，对当前 README 通过；
  删改表格任一码即红。
- `cargo audit --deny warnings` 本机实测 exit 0（cargo-audit 0.22.2，112 个依赖）。
- CI 变更无法在本机验证，需下一次 push 后看 run 结果（尤其 MSRV job 与
  `install-action` 步骤）。

### 批次三：Web UI 加固

#### Fixed

- **UI 存储型 XSS**（`ui/index.html` + `cli/ui/index.html`，两份同步）：`esc()` 只转
  `&` `<` `>`，**不转引号**，而 `tplCardHtml` / `fieldHtml` / `renderMachineSel` 等处的
  插值点连 `esc()` 都没调。于是模板文件名、`{# PARAMS: #}` 里的参数名、机床 TOML 的
  vendor/model 中只要有一个 `"`，就能逃出属性边界加出事件处理器
  （`data-tpl="a"onerror=alert(1) x="`）—— 打开 `nctool ui` 即执行。
  `esc()` 补 `"` `'`，并逐点补全全部插值位置（属性值与文本两处都要）。
  另新增 `selEsc()`：`querySelector('[data-param="…"]')` 的转义规则与 HTML 不同
  （要转 `\` 和 `"`，不是转成 `&quot;`），写入侧用 `esc`、回读侧用 `selEsc`，
  两侧才指向同一个 DOM 属性值 —— 否则「名字带引号的参数」会静默收集不到。

- **目录符号链接的逃逸与成环**（`cli/src/context.rs`）：`Path::is_dir()` 跟随符号
  链接，而逃逸校验（`canonicalize` + `starts_with(root)`）此前**只在文件分支**执行，
  目录层既无校验也无 visited 集合。现目录分支同样校验，并用 `BTreeSet<PathBuf>`
  记录已进入的规范化目录以断开环。

- **server 模式数字选项发成字符串**（`ui/index.html`）：`input.value` 是字符串，
  `normalizeOpts` 此前只在 demo 分支被调用，server 分支直接 `JSON.stringify` 发出，
  而后端按 `as_u64()` 取值 → 在「步进」框里改一个数字就「渲染失败」。
  输入处理器改存 `Number`，`API.render` 出口再统一过一次 `normalizeOpts`。

- **清单未提及的模板被静默隐藏**（`core/src/manifest.rs`）：`TemplateMeta` 用
  `#[derive(Default)]`，给出 `visible = false` / `output_extension = ""`，而 serde
  路径（`default_true` / `default_extension`）给出 `true` / `".NC"` —— 同一个「缺省」
  两处含义不同。`resolve` 对清单未提及的模板走 `unwrap_or_default()`，拿到的正是
  派生那版：往 `templates/` 放一个新 `.j2` 而不加清单条目，用户列表里看不到它，
  输出文件名还丢扩展名，且全程不报错。改手写 `impl Default`，与 serde 对齐。
  （报告把它归在 P1-4、未指定批次；因与批次一「静默出错」同类，随本批次一起做。）

- **HTTP 分类过滤缺「切槽」**（`cli/src/server.rs`）：`parse_category` 只有五个分支，
  而 `cli/src/cli.rs::CategoryArg` 有 `Grooving` —— `?category=grooving` / `?category=切槽`
  被 **400 拒绝**，CLI 侧却正常；前端 `CATS` 也缺这一项，切槽模板只在「全部」里出现、
  分类计数永远对不上。补齐分支，并遍历 core 全部分类加测试防再漏。
  另：`?category=`（空值）此前同样 400 —— 前端清空分类筛选就会撞上，现视为「不筛选」。

- **`options.format` 类型错误静默回退**（`cli/src/server.rs`）：`and_then(as_str)` 把
  「键缺失」与「类型错误」混为一谈，`{"format": 1}` / `true` / `[]` 都落到 None 分支
  被**静默按 gcode 生成**，调用方以为选项生效。同函数的 `get_bool` / `get_u32` 早已
  对类型错误返回 400，此处是唯一漏网。现区分二者，类型错误 400。

- **API 响应与错误体泄露绝对路径**（`cli/src/context.rs`、`cli/src/server.rs`）：
  文件模板的兜底描述是 `format!("文件模板: {}", canonical.display())`，经
  `/api/templates` 原样返回给浏览器 —— `curl` 一次就拿到全部模板的绝对路径（含用户名
  与项目结构）。现改用**相对键**（也正是用户要传给 CLI 的那个名字，比路径更有用）。
  500 响应同样不再回显内部正文（`build_registry` 的失败消息含绝对路径），改为泛化
  文案 + 详情写 stderr；400 仍回显（那是调用方能据以修正的模板语法/行列号，且无路径）。

#### Changed

- **`cli/src/server.rs` 模块头与 CSP 注释同步**：模块头仍写「非回环由命令层打印警告
  （本模块不做判断）」，而 `listen_addr` 早已**直接拒绝**非回环地址 —— 照那句话改回去
  会把这个决定悄悄撤销，故更正并写明沿革。CSP 注释补上「`'unsafe-inline'` 这条取舍
  成立的前提是页面里没有可注入的插值点」，以及改前端插值时必须一并复核。

#### 未做（附理由，非遗漏）

- **CSP 去掉 `'unsafe-inline'`**：内联 `<script>`/`<style>` 需外置为同源文件
  （或每响应注入 nonce），属独立改造。本轮先消除注入点本身。
- **body 读超时 / 并发上限（P1-19）`、每请求全树 `stat`（P1-18）**：tiny_http 0.12
  不暴露底层 socket，`set_read_timeout` 无从下手；换独立线程读又要把 `Request` 移进
  线程才满足 `'static`，而响应必须由持有 `Request` 的一方发出 —— 得重做 `serve`
  的请求循环。两者同属服务层改造，宜单独排期，不夹在安全修复里。

#### 测试

- 新增 6 项测试：`core` 1（清单缺省与 serde 对齐）、`cli` 3（分类覆盖全 core 变体 /
  空分类不筛选 / format 类型错误）、`cli` 2 项 `#[cfg(unix)]`（目录链接成环被断开、
  逃逸目标被跳过）。
- **浏览器实测**（非仅单测）：起 `nctool ui` 后在真实页面验证 ——
  ① 参数名含 `x"onmouseover="window.__XSS_ATTR=1` 的模板：无注入元素、属性回读
  与原名一致、`querySelector` 能回找到该输入框并收走其值；同页面用**旧** `esc()` 拼
  同样的 HTML 则确实注入 → 证明漏洞真实且已修。② 「步进」改 25 → `N0025/N0050/N0075`，
  渲染成功（此前 400 渲染失败）。③ 32 个模板正常列出、参数表单与校验提示正常。
- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`
  干净；workspace 全量通过：**540 项**（Windows，2 项 `#[cfg(unix)]` 不参与）/ 542 项（unix）。

### 批次四：一致性清理

#### Fixed

- **同名文件模板被静默替换为注册表模板**（`cli/src/commands/render.rs`）：模板目录里
  已有 `a.j2` 时，`nctool --template-dir templates render /tmp/other/a.j2` 会**渲染
  `templates/a.j2` 的 G-code** —— 用户以为渲染的是自己给的文件。原因：命中同名时
  直接 `return` 复用注册表条目，而注册表里那个是**另一个文件**。不报错、不告警，
  只是产出另一份程序。现改为：路径指向的就是注册表里那个文件时照旧复用；
  否则把用户给的文件注册进去，注册名退化为完整路径（不可能与目录模板名冲突）。
  反向验证：还原旧逻辑后新测试 FAILED。

- **`validate --format json` 遇断管道 panic**（`cli/src/commands/validate.rs`）：该分支
  用 `println!` 输出，是全仓**唯一**绕过 `write_stdout_quiet` 的地方。下游提前关管道
  （`nctool validate <大模板> --format json | :`）时 `println!` 以 panic 收场 ——
  实测退出码 **101**，而退出码矩阵承诺的是 1。现走 `write_stdout_quiet`
  （已改为 `pub(crate)`），实测退出码 **1**。

- **include 闭包的同名规格取了最远的那份**（`core/src/registry.rs`）：
  `collect_include_closure` 先递归进被引用模板、再并入它自己的规格，而合并策略是
  「先到先得」—— 于是 `main → child → grandchild` 且 child 与 grandchild 对同名参数
  声明了不同 `options` 时，采用 **grandchild** 的，与文档承诺的「更接近主模板的声明
  优先」相反。不报错，只是静默按另一套约束校验。现改为先入表、后递归。
  反向验证：还原旧顺序后新测试报 `left: [grandchild]`，确认复现报告所述。

- **清单孤儿条目零检测**（`core/src/manifest.rs`、`cli/src/context.rs`）：清单里写了、
  但没有对应模板文件的键，其 `params`（白名单/区间）、`visible`、`machine`、
  `output_extension` 全部**静默失效** —— 看起来约束齐全，实际一条都没上。写成
  `turning/undercut.j2`（实际是 `undercut_fs.j2`）这类笔误尤其容易发生。
  项目已为「规格写了个不存在的参数」设了 `SpecInert` 警告，此处是对称的补口：
  新增 `TemplateManifest::orphan_keys`，注册表加载时对未命中键打 `warning:`（不阻断）。
  本仓库自身的清单实测无告警。

- **稀疏覆盖只能设、不能清**（`core/src/manifest.rs`）：`ParamOverride` 的约束字段
  是 `Option<T>`，一旦从 `variables.yaml` 或头部继承到值就回不到 `None`——变量库给
  `U_Q` 声明 `min: 0` 后，**所有**模板都被套上，某模板确实需要负值也只能去改变量库，
  那会波及全部模板；继承来的 `derive` 同理，想关掉没有别的办法。
  现把 `default` / `min` / `max` / `unit` / `options` / `required_if` / `derive`
  改为 `Option<Option<T>>`（配 `double_option` 反序列化），三种写法语义区分开：
  **没写** = 沿用继承值、**写 `null`** = 清空该条继承、**写值** = 设值；
  `options: []` 与 `options: null` 等价。`templates/README.md` 与
  `docs/ARCHITECTURE.md` / `docs/SYSTEM_DESIGN.md` / `docs/ARCHITECTURE_REVIEW.md`
  同步更新（原先都写作「字段全为 `Option`，能区分没写与写成默认值」）。

- **数值过滤器的三处边界**（`src/filters.rs`）：
  - `nc_fixed` / `nc_strip` 不归一负零：`-0.0 | nc_fixed(3)` → `-0.000`、
    `-0.0 | nc_strip` → `-0`。`nc_signed` 早已归一，同一份逻辑换个过滤器就输出
    不同字节，而控制器对负零的处理并不一致。现三个过滤器统一归一。
  - `nc_signed` 的归一放在**舍入之前**：`-0.0001` 保留 3 位就是 `0.000`，
    判零漏掉它，仍输出 `-0.000`，与本节「-0.0 归一到 +0.000」的承诺相悖。
    改为按小数位定值后再判零（用字符串判零，不再做一次浮点运算 —— `-0.0005`
    这类值在「乘再除」里会抖到另一侧）。
  - `nc_pad` 上界检查差一：`i64::MAX as f64` 恰为 2^63（`i64::MAX` 本身在 f64
    里不可表示），用 `>` 会让 2^63 通过检查、被 `as i64` **饱和**成 `i64::MAX` ——
    静默输出一个错误的程序号，正是该检查要拦的东西。`>` 改 `>=`。
  - `nc_pad` 的文档说「输入为浮点数时截断小数部分取整」，实现却是**拒绝**小数
    （截断会让 `n=1.7` 静默产出 `N0001`）。改文档，并在 `nc_fixed` 上写明
    **舍入模式是就近取偶**、与 minijinja `round` 的「半远离零」在 .5 附近相反 ——
    前者是对齐源项目 Python `f"{v:.2f}"` 的有意选择，要统一语义请改模板而非改这里。

#### Changed

- **`BUILTIN_GLOBALS` 只列 minijinja 真正提供的全局**（`src/extract.rs`）：名单里此前
  有 `lipsum` / `cycler` / `joiner` —— 那是 Jinja2（Python）侧的东西，minijinja
  不提供，本库也没有 `add_global` 注册它们。后果不是「多排除几个名字」，而是
  **恰好叫这些名字的模板参数**被静默排除出未声明集合，必选与类型校验一起失效；
  而真去写 `{{ lipsum() }}` 的模板在渲染期照样报未定义，名单一点忙没帮上。
  新增测试用 `minijinja::Environment` 实证这三个名字渲染即报错。

- **`{type: integer}` 的超范围值不再静默饱和**（`core/src/model.rs`）：`as i64` 对
  超范围值返回 `i64::MAX` 而不是报错，于是 `{type: integer, value: 1e20}` 被换成
  9223372036854775807 —— 一个完全不同的数，而 `is_finite() && fract() == 0.0`
  两道检查都放行。现过界报错；`ParamValue::as_integer` 对超界 `Number` 返回 `None`
  而不是饱和值。（`i64::MAX as f64` 恰为 2^63，合法区间是 `[-2^63, 2^63)`。）

- **NaN 不再额外刷一条 `NotInteger`**（`core/src/validate.rs`）：`NaN.fract()` 仍是
  NaN，而 `NaN != 0.0` 为真，整数约束照样命中 —— 同一个值报两条错
  （NonFinite + NotInteger）。既是噪声，也把宽松模式「唯一硬失败项 = NonFinite」
  这个承诺搅浑。区间检查不需要这层保护（NaN 与任何数比较都是 false）。

- **`{# PARAMS: #}` 块超限不再静默截断**（`core/src/manifest.rs`）：块在第 200 行
  仍未闭合时，其后的参数声明被**静默丢弃** —— 类型/白名单/条件必选全不生效，
  而用户只当模板本来就没写。现产出一条告警。

- **`ManifestFile` 补 `deny_unknown_fields`**（`core/src/manifest.rs`）：带 `templates:`
  的形式只有这一个合法顶层键，此前多写/拼错会被静默忽略 —— 用户以为改的是清单、
  实际什么也没改。`from_yaml` 的注释一直声称有这条保护，但结构体上没有。

- **`check_docs_links.py` 接入 CI**（`.github/workflows/ci.yml`）：`CONTRIBUTING` §3
  早已文档化其用法，却始终没有调用点 ——「工具闲置」等于这道门不存在。脚本对坏链接
  exit 1（已实测），可直接当门禁；与对拍脚本一样只在 ubuntu 跑一次。

- **两处与实现不符的注释**（`src/extract.rs`、`cli/tests/cli_e2e.rs`）：前者断言
  「minijinja 内部自带 JIT 编译缓存」，与 `render` 每次都走一遍
  `template_from_named_str` 的实际调用形式对不上，属无据的说法，改为陈述本项目
  自己的不变量；后者写「本仓库根目录下唯一的临时目录」而实现用 `std::env::temp_dir()`，
  且同一段注释的下一句就在说「不污染仓库」。

#### 测试

- 新增 5 项：`explicit_path_that_collides_with_registered_name_wins`（含反向用例：
  同一文件不该改名）、`include_closure_spec_precedence_is_nearest_to_main`、
  `orphan_keys_reports_unmatched_entries`、`manifest_null_clears_inherited_field`、
  `template_refs_traverse_every_nested_body`（11 个分支的表驱动用例，见下）。
- 新增 4 项数值过滤器用例（`src/filters.rs` 此前**没有测试模块**）。
- 新增 5 项：`builtin_globals_are_exactly_what_minijinja_provides`（含 minijinja 实证）、
  `oversized_integer_is_rejected_not_saturated`、`nan_does_not_also_report_not_integer`、
  `manifest_rejects_unknown_top_level_key`、`params_block_beyond_scan_limit_warns`。
- **反向验证**：上述各项均临时还原实现后确认 FAILED 再恢复。数值过滤器三项的失败
  输出分别是 `left: "-0.000"` / `right: "0.000"`、`left: "-0.000"` /
  `right: "+0.000"`、以及 2^63 未报错。
- **P1-20 覆盖缺口**：`collect_template_refs_stmt` 的嵌套语句体分支此前零覆盖
  （lcov 显示 for-else、if 的 else 体、with / set-block、autoescape / filter-block /
  block / macro / call-block 全部 `DA:...,0`）。该函数是**组合模板必选参数不漏检**
  的入口，漏一个分支 = 用那种语法写的子模板参数完全不校验。补齐后 `src/extract.rs`
  第 158–202 行已全部覆盖，生产口径从 88.65% 升至 **89.49%**。
  实现在补齐前即为正确，本项是纯覆盖缺口。
- 孤儿键告警用真实 CLI 验证（临时模板目录 + 清单里一条拼错的键）：
  `templates list` 打出一条 `warning: 清单条目 turning/undercut.j2 未匹配到任何模板文件…`，
  且只报未命中的那条。
- workspace 全量 **554 项**通过（含 golden 逐字节比对 —— 无模板依赖旧的负零输出）。

### 批次五：打通扩展接缝（第四轮审查 P0-1）

> 依据 `docs/CODE_REVIEW_2026-09-19.md`。本批**不改变任何运行时行为**，只把
> 「新增枚举变体时编译器是否会提醒你漏改」从事后排查变成编译期错误。

#### Changed

- **`ParamKind` 的类型名解析收敛到唯一入口**（`core/src/model.rs`）：新增
  `impl FromStr for ParamKind`（接受 `aliases()` 的英文别名与 `label()` 的中文名），
  清单解析处的那份字符串表（`core/src/manifest.rs::parse_kind_name`）改为转发。
  原先同一套中英文类型名在两处各写一遍，新增类型漏改一处即「模板头部写了但报未知类型」。
- **`ParamKind::matches` 去掉 `_ => false` 兜底**（`core/src/model.rs`）：原写法是
  `match (self, value)` + 通配兜底，新增变体会**静默把新类型变成"拒绝一切取值"**，
  编译器一声不响。现按 `self` 穷尽展开，新增变体必须逐类表态。语义与原先逐值一致。
- **`OutputFormat` 后处理分支改 `match`**（`core/src/pipeline.rs`）：原先是
  `if opts.format == OutputFormat::Text { return }`，新增第三种格式会静默落到
  G-code 后处理（加行号 / 清 ASCII）里。现为穷尽 `match`。
- **宽松模式的保留集合改由 `IssueKind::is_hard_fail()` 决定**（`core/src/validate.rs`）：
  新增穷尽匹配的 `is_hard_fail()` 与 `ValidationReport::downgrade_soft_errors()`，
  管线改走新路径。原 `downgrade_errors_except(&[NonFinite])` 是**白名单反向**，
  新增问题类别会被默认降级；该方法保留但文档指向新入口。
- **分类表收敛到 core**（`core/src/registry.rs`、`cli/src/server.rs`、`cli/src/cli.rs`）：
  新增 `TemplateCategory::ALL` / `aliases()` / `dir_names()` / `from_dir_name()` /
  `FromStr`。HTTP 的 `parse_category` 与清单的 `classify_by_path` 均改为转发，
  `CategoryArg::from_core` 改为直接取 `TemplateCategory::label()`（原先是第三份中文副本）。
  `classify_by_path` 的文档表补上此前遗漏的 `grooving` / `groove`。

#### Fixed

- **golden 刷新命令漏 `--workspace`**（`README.md`、`docs/CONTRIBUTING.md`）：文档写
  `NCTOOL_UPDATE_GOLDEN=1 cargo test`，而 golden 测试在 `core` 包 —— 裸 `cargo test`
  只跑根 crate，命令成功、**零文件变更、且不报错**，正好踩中本项目反复警告的
  workspace 陷阱。现补 `--workspace`。

#### 测试

- 新增 3 项守卫：`param_kind_registry_is_complete`（`ALL` 长度与穷尽匹配的变体数对拍
  + 中文名/全部别名可解析回自身 + 别名表非空）、
  `downgrade_soft_errors_keeps_hard_fail_only`（`is_hard_fail` 逐类断言 + 与旧白名单
  路径行为等价）、`category_arg_covers_every_core_category`（`CategoryArg::value_variants()`
  与 `TemplateCategory::ALL` 对拍）。
- **反向验证**（临时改坏实现 → 确认 FAILED → 恢复）：把 `NonFinite` 的 `is_hard_fail`
  改成 `false` → 守卫测试报「NaN/Inf…也必须阻断」；删掉 `enum` 别名 → 既有解析测试报
  `'enum' 既不是已知类型`；`ALL` 少登记一个变体 → 分类守卫报 `left: 6 / right: 5`。
- workspace 全量 **557 项**通过，`cargo fmt --all -- --check` 与
  `cargo clippy --workspace --all-targets -- -D warnings` 均通过。

### 批次六：前后端接口集合对拍（第四轮审查 P0-2）

> 依据 `docs/CODE_REVIEW_2026-09-19.md` §4 批次 B 第 1、2 项。

#### Changed

- **`runBatch` 不再直接调 `API.mock`**（`ui/index.html`、`cli/ui/index.html`）：改为走
  `API.partGenerate` 出口。此前服务模式下会绕过后端、用前端 JS 复刻的引擎产出 G-code，
  而后端 `/api/part/generate` 并不存在。demo 模式（`file://`）仍落到 mock，行为不变。
  > 复核更正：该入口在服务模式本就被 `API.mode` 守卫挡住（不可达），故实际风险是
  > 「潜伏的契约漂移」而非「静默产出假程序」；报告已相应降级为 P1。

#### Added

- **`scripts/api_routes.json`**：前后端接口集合的**单一来源**（7 条后端路由 + 1 条
  `frontend_only` 显式豁免）。新增端点必须同时改三处：后端 `route()` 路由臂、本文件、
  前端 API 封装，漏改任一处即红。
- **`scripts/check_api_parity.mjs`**：提取两份 HTML 里所有 `/api/...` 字符串字面量，
  断言每一个都在 fixture 中登记；`frontend_only` 豁免项必须写明 `reason`（防无限期豁免）。
  已接入 `.github/workflows/ci.yml`，与 param 对拍并列（仅 ubuntu 跑一次）。
- **Rust 侧守卫 `api_routes_are_routable`**（`cli/src/server.rs`）：`include_str!` 消费
  同一份 fixture，逐条断言 `route()` 不返回「未知接口」。只看是否未知，不校验状态码
  —— fixture 用最小请求体，400（参数不全）是正常的，404 + 未知接口才是契约漂移。

#### 测试

- **双向反向验证**：删掉 fixture 里的 `/api/part/generate` 豁免 → 前端门禁报
  「前端使用了 /api/part/generate，但未登记」×2；往 fixture 加一条后端没有的接口 →
  Rust 测试报 `route #7: GET /api/nonexistent_probe 未登记到 route()`。
- workspace 全量 **558 项**通过；`node scripts/check_api_parity.mjs` 通过；
  `ci.yml` YAML 解析校验通过。

### 批次七：三处静默正确性（第四轮审查批次 C）

> 依据 `docs/CODE_REVIEW_2026-09-19.md` §3.2 的 P1-1 / P1-2 / P1-3 / P1-11。

#### Fixed

- **`ParamKind::Integer` 补 i64 范围检查**（`core/src/model.rs`）：`matches` 此前只判
  `is_finite() && fract() == 0.0`，而 `1e20` 两条都满足 —— 校验放行，但 `as_integer`
  会把它**饱和**成 `i64::MAX`。同一文件的 `coerce_tagged` 与 `as_integer` 早已有
  `[I64_MIN, I64_MAX)` 守卫（第三轮 P2-6 修的），唯独类型检查漏掉。现与它们同口径。
- **宽松模式的派生失败并入硬失败**（`core/src/validate.rs`、`core/src/pipeline.rs`）：
  `DeriveFailed` 是 Error 级，但宽松模式此前把它降级成 Warning，报告说"可以出程序"，
  紧接着 `derive::apply` 又返回 `Err` —— 报告被丢弃、调用方一个字都拿不到，而文档
  声称"唯一硬失败是 NaN/Inf"。现：
  - `IssueKind::is_hard_fail()` 把 `DeriveFailed` 并入（与它自己的级别说明一致）；
  - 新增 `ValidationReport::has_hard_fail()`，管线据此判定，不再写死 `NonFinite`
    —— 将来新增硬失败类别不必改调用点；
  - `generate_lenient*` 的文档承诺同步改为"两类"。
- **清单键规范化碰撞不再静默覆盖**（`core/src/manifest.rs`、`cli/src/context.rs`）：
  `./turning/a.j2` 与 `turning/a.j2` 归一后同键，后者覆盖前者且不报错 —— 用户以为
  两个条目都在生效。`TemplateManifest` 现记录冲突对（`duplicate_keys()`），
  CLI 加载时与孤儿键一并打出 warning。对照变量库对重复变量是直接报错的。
- **有限性闸门改为 fail-closed**（`core/src/registry.rs`）：`find_non_finite` 里的
  `try_iter().ok()?` 会把"遍历失败"折叠成"没找到非有限数"，即**防 NaN 的最后一道
  闸门在异常路径上静默放行**。现返回 `Result<Option<String>, String>`，
  `ensure_finite_context` 对扫描失败同样拒绝渲染。

#### 测试

- 新增 3 项：`integer_kind_rejects_out_of_i64_range`、
  `generate_lenient_treats_derive_failure_as_hard_fail`、
  `normalized_key_collision_is_reported`；并强化
  `downgrade_soft_errors_keeps_hard_fail_only`（补 `has_hard_fail` 与
  `is_hard_fail` 同口径断言）。
- **反向验证**：临时还原四处实现 → 4 项测试全部 FAILED（`integer_kind_…`、
  `downgrade_soft_errors_…`、`generate_lenient_treats_…`、`normalized_key_…`）→
  恢复后全绿。
- **顺带修掉一个此前潜伏的 CI 红**：`RUSTDOCFLAGS="-D warnings" cargo doc` 报
  「links to private item」（`ParamKind::aliases`、`TemplateCategory::dir_names`、
  `Self::aliases`）与 CLI 侧未解析链接 —— 批次五引入的文档链接问题，
  当时只跑了 fmt/clippy/test，没跑 doc。现已修正。
- workspace 全量 **561 项**通过；`cargo fmt --all -- --check`、
  `cargo clippy --workspace --all-targets -- -D warnings`、
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` 均通过。

### 批次八：让 golden 基线重新有信息量（第四轮审查批次 B 第 3 项）

> 依据 `docs/CODE_REVIEW_2026-09-19.md` §3.1 的 P0-3。

#### Added

- **3 组负向 golden**（`tests/golden/neg_*.report.txt`）：`neg_missing_required`
  （drill_cycle 只给 x）、`neg_type_mismatch`（tool_num 传字符串）、
  `neg_out_of_range`（prog = 99999 越界），由新用例
  `negative_cases_freeze_failure_reports` 冻结。此前 21 份报告恒为
  「校验通过：无问题」，**报告维度只有 1 份信息量**，把 `Missing` 误标成警告或
  把消息里的参数名写错不会有任何测试发现。现在失败路径的级别、参数名、
  行号定位与文案全部纳入基线。
- **`machine_dimension_is_currently_flat`**：显式守住"机床维度当前不产生差异"。

#### 说明：为什么没有去"制造"机床差异

复核后确认 21 份 `.nc` 只有 7 份不同内容**不是测试写错**：三个预设只在
`max_spindle_rpm` / `machine_type` / `axes` / `vendor` / `model` 上不同，而这些键
**没有任何内置模板引用**；模板真正用到的 `program_prefix` / `units` /
`coordinate_system` / `feed_mode` 等全部来自共享的 `generic_config()`。
要让输出分化只能凭空发明机床编程约定 —— 那不是测试修复，是伪造数据。

故改为把这份偶然的重复变成**受守的断言**：任一预设改动模板可见的键，该用例即红
并提示"机床维度开始分化，请改成真正分维并复核 `_wfl` / `_index` 基线"；
它同时拦住"为了消重而删掉重复基线"——删了就没有东西能拦住预设的模板可见改动。

#### Changed

- 修正 `core/tests/integration.rs` 的过期注释（"6 内置模板 × 3 = 18 组"实为
  7 × 3 = 21）与 `golden_files_are_lf_only` 的文件计数（42 → 45）。
- 同步 `README.md` / `docs/PROJECT_STATUS.md` / `docs/ROADMAP.md` 的 golden 数量。

#### 测试

- **反向验证**：临时改掉校验报告里 `Missing` 的文案 → `negative_cases_freeze_failure_reports`
  FAILED；临时把 `IndexMs40` 的 `coordinate_system` 改成 `G55` →
  `machine_dimension_is_currently_flat` FAILED。恢复后全绿。
- 用刷新模式重建基线后 `git status tests/golden/` 只显示 3 个**新增**文件，
  既有 42 份**逐字节未变** —— 反向证明提交的基线是可复现的。
- workspace 全量 **563 项**通过；fmt / clippy / doc 门禁均通过。

### 批次九：去重与测试诚实性（第四轮审查批次 D 的一部分）

> 依据 `docs/CODE_REVIEW_2026-09-19.md` §3.2 的 P1-7 与 §3.3 的 P2-10 / P2-14 / P2-33。

#### Changed

- **校验报告的 JSON 形状收敛到 core**（`core/src/validate.rs`、`cli/src/output.rs`）：
  新增 `ValidationReportJson` / `ValidationIssueJson` 与 `ValidationLevel::as_str()`，
  CLI 侧只保留 `output::report_json` 一个薄封装。此前
  `cli/src/server.rs::validation_json` 与 `cli/src/commands/validate.rs::report_json`
  **逐字段各写一遍**（连 `level` 的三分支映射都各写一份），两份漂移会让 Web UI 与
  CLI 对同一份报告给出不同形状，而两边都没有测试能发现对方变了。
- **机床清单枚举收敛到 core**（`core/src/machine.rs`）：新增 `MachineEntry` 与
  `MachinePreset::entries()`，把「预设 + 配置文件自定义、按预设 id 去重」的规则
  从 HTTP（`server.rs::machines_list`）与 CLI（`commands/machine.rs::list`）两处
  合并为一处；展示字段仍由调用方决定（HTTP 带完整 `config`，CLI 走
  `MachineEntry::display_line`）。
- **删掉两处死代码**（`src/extract.rs`）：`c.declare("loop")` —— `record` 对
  `RESERVED_NAMES`（含 `loop`）在入口即 return，早于任何 `is_local` 查询，
  该调用可证明无任何可观察效果；以及 `Collector::new` 从未使用的 `_src` 参数。

#### 测试

- **`all_math_filters_render` 由弱断言改为精确断言**（`tests/parsing.rs`）：原实现把
  9 个过滤器拼成一行再 `assert!(out.contains("2"))`，`"2"` 可由 `sqrt(4)` /
  `log10(100)` / `ceil(1.5)` 任一个满足，最后两条还重复检查同一个 `"2"`。
  改为逐个按数值比较后**立刻抓到 `sqrt(4)` 渲染成 `"2.0"` 而非 `"2"`** ——
  旧断言之所以"通过"，正因为它只看子串。
- **fuzz 补返回值不变量**（`src/lib.rs`）：`fuzz_random_parse_extract_no_panic` 此前是
  `let _ = extract_variables(&ast);`，只验证"不 panic"，易造成"提取器已被 fuzz 验证"
  的错觉。现补：变量名非空、`extract_variables` 按名去重、行列从 1 起、span 有序、
  `undeclared ⊆ all`。
- 新增 2 项守卫：`json_view_freezes_contract_fields`（冻结报告 JSON 的字段名与
  `level` 取值，属对外契约）、`entries_lists_presets_then_custom_without_duplicates`
  （重名的自定义机床必须被忽略，否则列表出现两个 `generic`）。
- workspace 全量 **565 项**通过；fmt / clippy / doc / 接口对拍门禁均通过。

#### 未做（附理由，不是遗漏）

- **服务层读超时 + 并发上限**：tiny_http 0.12 不暴露底层 socket，根治需重做 `serve`
  的请求循环，宜独立排期。
- **每请求全树 `stat`**：简单改成"只 stat 目录"会**漏掉就地编辑文件内容**
  （父目录 mtime 不变），缓存将返回过期模板；需 TTL 或显式刷新入口的设计取舍。
- **`extract.rs` 两套并行遍历合并**：**有意不做**。该文件最安全敏感（必选参数漏检
  = 撞刀级静默错误），合并属"改对了没收益、改错了很难发现"的重构。
- **`derived_names` / `invalidate_analysis` 可见性收窄**：`nctool-core` 已发布到
  crates.io，收窄 `pub` 是破坏性变更，并入下一个 minor 版本一起做。

### 批次十：收尾小项（第四轮批次 E 的一部分）

#### Fixed

- **`ui --open` 不再先于绑定**（`cli/src/commands/ui.rs`、`cli/src/server.rs`）：
  `serve` 拆成 `bind` + `serve` 两步，先绑定、失败即返回 `Err`，再打印横幅与开浏览器。
  此前端口被占用时会先弹出一个指向死页的浏览器标签页。
  同时 `bind` 回读 `server.server_addr()` 拿**实际**端口 —— `--port 0` 时内核分配端口，
  旧代码的横幅与浏览器 URL 都指向 `:0`。
  实测：`--port 0` 显示真实端口；端口占用时退出码 3 且不弹浏览器。
- **错误链截断留痕**（`src/error.rs`）：超过 `MAX_ERROR_CHAIN`（8 层）时追加
  `…（错误链超过 8 层，已截断）`。静默丢弃根因会让"最后一层"看起来就是原因，
  而它其实只是第 8 层。
- **`RegistryError::Io` 带上路径**（`core/src/registry.rs`）：该变体只装 `io::Error`，
  用户此前只看到「文件读取失败: 系统找不到指定的文件」，不知道是哪个文件。
  变体形状是已发布 API 不能改，故在构造处把路径并进 `io::Error` 的消息。
- **`release.yml` 补 `--locked` / `--all-targets`**：与 `ci.yml` 对齐。
- **文档残留的"15 组 golden"**（`PROCESS_CHECKLIST` / `ROADMAP` 共 3 处）改为 21 组正向；
  `DEV_PLAN_CLI_UI` 的 `cargo clippy` 补 `--workspace`。

#### Changed

- **CLI 的 golden 测试改为读共享基线**（`cli/tests/cli.rs`）：3 个测试此前各自硬编码
  一份期望输出，测试名却声称"与 nctool-core 管线逐字节一致"—— 模板一改要手工同步
  两处，且没有任何测试能发现两份已经不一致。现新增 `read_golden()` 直读
  `tests/golden/*.nc`；`program_header` 的参数对齐 fixture（补 `part_name=DEMO`），
  另保留一条"省略可选参数走规格默认值"的断言。
  **反向验证**：改动 golden 基线后 CLI 侧与 core 侧**同时变红**；此前 CLI 侧照样绿。

#### 测试

- 新增 3 项：`bind_with_port_zero_reports_actual_port`、
  `bind_reports_error_instead_of_panicking`、`add_file_io_error_includes_the_path`。
- workspace 全量 **568 项**通过；fmt / clippy / doc 门禁均通过。

#### 仍开放

- UI 的 P2-25（Bool 参数恒提交 `false`）与 P2-26（缺 `AbortController`）：两者都需
  浏览器验证，而本机不支持 agent-browser（Windows），盲改不符合本项目"改动须实测"的标准。
- `src/lib.rs` 1760 行内联测试迁移：需先确认它们是否只用公共 API，收益不明确。

#### 一处更正

第四轮报告 §3.3 引用的「`src/renderer.rs:185` 多克隆一次源码」经复核**不成立**：
`name` 与 `source` 在紧随其后的 `.map_err(|err| from_minijinja_error(err, &name, Some(&source)))`
里仍被借用，而 `add_template_owned` 按值接管两者，两个 `clone()` 都必需。
报告已同步更正，**不要按原文去"修"**。

---

## [nctool-tpl 0.4.0] · [nctool-core 0.3.0] · [nctool-cli 0.3.0] - 2026-09-18

「NCTool_V3 模板资产整合 + 参数规格系统 + 架构评估 P0/P1 收口」

三个 crate 同步发版（tpl 0.3.2 → 0.4.0、core 0.2.2 → 0.3.0、cli 0.2.2 → 0.3.0）。

**关于本节的分节方式**：本轮改动在 crate 之间高度交织——「文件模板的参数规格外部化」
同时改 `core/src/manifest.rs` 与 `cli/src/context.rs`，「渲染入口的有限性闸门」同时改
`core/src/registry.rs` 与 `nctool-tpl` 的再导出，覆盖率门禁/CI 则不属于任何单个 crate。
强行按 crate 切分会让同一条改动被拆到两处或被迫归错。故本节**沿用按主题分节**，
每条条目内均标注涉及的文件路径，便于按 crate 检索。三个 crate 的 tag 分别是
`nctool-tpl-v0.4.0` / `nctool-core-v0.3.0` / `nctool-cli-v0.3.0`。

### Added

- **`inspect` 展示参数规格**（`cli/src/commands/inspect.rs`）：规格（类型 / 候选值 /
  区间 / 整数 / 条件必选 / 派生）在 `validate` / `render` 时是**真的会拦人**的，
  此前却只能靠"触发一次报错"来发现。现在 `inspect` 直接列出来，并按
  **必选 / 条件必选 / 派生 / 可选**四桶分组——规格引入后必选性不再只有两态，
  混在两桶里会误导用户去填一个不该填的参数：
  - 每行：`名字  行 L 列 C  类型  约束摘要  描述`；候选值超过 6 个折叠为 `…（共 N 项）`
  - 末尾提示"未标注类型（`ParamKind::Any`）"的参数——这类参数不做类型检查，
    写错要到渲染期才暴露
  - JSON 输出的规格字段**复用 HTTP API 的 `spec_json`**，避免两处形状各自漂移

- **`--param k=v` 按规格归一取值**（`cli/src/args.rs`）：argv 里没有类型信息，此前
  只按"像不像数字"推断（`5010` → 数值）。字符串型参数若值恰好形如数字
  （`U_CTB` 的 `1631`、`U_ID` 的 `[42]`）会被推断成数值 → 类型不匹配，用户只能改用
  `--params-file` 传 JSON 字符串或加 `k:s=` 后缀。现在按规格归一，
  规则是**先白名单、后类型**：候选值命中哪种解释就用哪种（`U_CTB` 的 `1631` 与
  `"DECKEL"` 各命中一半），都没命中则按声明的类型（`String` 保持字符串）。
  **显式后缀 `k:s=` / `k:n=` / `k:b=` 仍优先**——那是用户明确表达意图的通道。

- **Web UI 参数表单消费规格**（`ui/index.html` + `cli/ui/index.html`）：
  - **候选值参数渲染为下拉框**（此前是文本框，非法值要等校验才报错）
  - **派生参数移出表单**，改为底部一行"由系统派生（无需填写）：tip_depth（…）"——
    此前它以普通输入框出现，填了会被派生值覆盖
  - **条件必选参数标注触发条件**（`条件必选：side = "Right" 时必填`）
  - 表单取值按规格归一，规则与 CLI 的 `--param` 一致（前端 `coerceParamValue`）

- **`ui/index.html` 与 `cli/ui/index.html` 一致性测试**（`cli/tests/cli.rs`）：
  前者是 `file://` 演示版、后者被 `include_str!` 嵌进二进制（`nctool ui` 提供的那份），
  是同一份页面的两份拷贝。手工同步迟早漂移，届时"改了页面却看不到变化"很难查——
  加断言把漂移变成测试失败。

- **派生参数 `derive`**（`core/src/derive.rs`）：把**查表型换算**从模板搬到规格，落实
  「模板只做变量替换，计算在 Rust 侧完成」这条核心原则。首个用例是
  `machines/index_g420/dg_cal_ir9.j2` 的 `tip_model → tip_depth`（12 项表，回退 29.61 = DM24）——
  此前这张表写在模板里（`{% set tip_depth_map = {...} %}` + `map[k] | default(v)`，
  为绕开 minijinja 的 map 无方法限制，即约定 R6），现在数据表**只此一份**（`variables.yaml`），
  改表只改 YAML：
  - 声明：`ParamSpec.derive: DeriveRule { from, table, fallback }`；YAML 侧与 `options` 一样
    走稀疏覆盖（`ParamOverride.derive`），因此变量库与清单 `params` 都能声明
  - **系统注入语义**：派生参数不要求调用方提供（与 `machine` 同类）；调用方提供了则
    **派生值恒胜**并报 `ShadowedSystemVar` 警告（派生值必须与源参数一致——表里说
    `DM24 → 29.61`、用户填 `10`，允许覆盖就等于允许"型号与深度对不上"的 G-code）
  - **失败不静默**：源参数缺失/未命中表项且无 `fallback` → 新 `IssueKind::DeriveFailed`
    （Error），**不取 0**（中心孔深度取 0 会让 `I_R9[80]` 顶紧位置算错）；且不再对同一
    参数叠报"缺失"
  - 求值时机：**校验前**（`check_vars`，两个校验入口共用）与**渲染前**（`pipeline` 两条路径），
    口径一致；顺序为**先派生、再规格默认值兜底**（源参数可能靠默认值才存在）
  - **派生值照常过类型/白名单/区间检查**——派生不是绕过校验的后门
  - 表键比较走 `ParamValue::matches_option`（数值/整数跨变体按数值相等），
    表键写 `8` 也能命中调用方传来的 `8.0`
  - 源参数为列表类型时明确报错（`UnusableSource`），而不是静默落到 `fallback` 把
    "传错类型"掩盖成"用了个看似合理的默认值"
  - `PipelineError::Derive` 兜底渲染路径（校验阶段已拦截，正常不可达）

- **变量库 `templates/variables.yaml`**（`core/src/variables.rs`）：**按变量名**生效的全局参数规格。
  同一变量在多台机床/多个模板上含义一致时只写一次，不必在 12 个模板里重复声明 `U_Q` 的候选值。
  内容从源项目 NCTool_V3 的 `configs/variable_repo.json`（62 个变量）导入：
  - **三级优先级**：清单 `templates.yaml` 的 `params`（本模板覆盖）> `variables.yaml`（全局）
    > 头部 `{# PARAMS: #}`（模板局部）。三者复用同一套稀疏覆盖机制（`ParamOverride` +
    `merge_params`），"只写要改的字段"这一约定在三个层级完全一致
  - **生效范围**：只对**模板确实引用了**的变量生效（头部声明的 + 源码里引用到的），
    因此库里可以放心放全局变量——没被引用的不会注入规格，也不会触发
    "规格声明了未引用参数"告警
  - 支持 `variables:` 键与顶层列表两种写法；**同名重复定义直接报错**（静默覆盖会让
    "改了定义却不生效"变成难查的问题）
  - **三条刻意的不导入**（都为了不产出"跑得通但错误"的 G-code）：
    ① **不导入默认值**——源库默认值是针对特定样件的预填值（`U_A = 141.25` 是那根轴的
    长度），规格默认值会让**缺参静默通过**，用户少填一个零件尺寸就会拿到另一根轴的程序；
    ② **不导入描述**——头部描述是写给这个模板的，源库描述以 YAML 注释保留；
    ③ **`read_only` 变量只导入类型、不导入候选值**——源库的 `read_only` 意为"由机床设置/
    派生计算决定"，其 `options` 是某次装夹的**取值快照**（`U_ANG: [20]`、`R1: [4000]`），
    当作用户可选集会禁掉合法的换刀/换料调整；真正由用户选择的变量（键槽宽度系列、
    槽宽系列、刀号、开口方向、顶尖型号）才导入 `options`
  - 导入后 19 个 INDEX G420 模板**无需改动头部**即获得类型与白名单（头部只写
    `name 必选 描述` 的形态由 `ParamKind::Any` 兜住）
  - 混有字母与数字的刀具标识（`U_CTB: 1631 / DECKEL`、`U_RTB: CT1017 / 5010`）
    用 `kind: any` + 混合候选项——数值与文本各自按值语义比较，使 CLI 的
    `--param U_CTB=DECKEL` 与 `--param U_FT=5010`（推断为数值）都能命中

- **`IssueKind::SpecInert`**：规格中存在**永不生效**的声明时报**警告**。两类成因：
  ① 规格声明了模板未引用的参数（参数名拼错，或模板改名后头部/变量库/清单未同步）；
  ② **约束与声明的类型不匹配**——`min`/`max`/`integer` 只对数值生效，
  `check_value_constraints` 对非数值类型在 `as_f64()` 处提前返回，声明在
  `String`/`Bool`/`List` 上时永不执行（`Any` 不算：它的值可能恰好是数值，约束按值生效）。
  与 `Unused`（用户多传了参数，无副作用）**必须区分**——混用会让两者都失去意义

- **`ParamKind::Any`**：表达"已声明但未标注类型"——不做类型检查，但 `options` /
  `required_if` / `default` 照常生效。存在的意义是让"只知道有这个参数、还不知道类型"的
  迁移模板也能参与白名单与条件必选；若强行猜类型会让合法输入被误拒，若不生成规格则连
  白名单都声明不了。**这是过渡态**，类型应从图纸/源变量库补齐

- **条件必选参数**（`ParamSpec::required_if` + `IssueKind::ConditionalSkipped`）：互斥分支参数
  （同一时刻只有一个分支可达）在静态变量提取下会被判为"全部必选"。实测 `undercut_fs.j2`
  只做右侧槽的用户被要求再填一个左侧专用 Z 值，历史规避手段是 `| default(0)`——而那会
  **静默产出 `Z0`**。现在把"哪个分支用到哪个参数"显式声明出来：
  - 判定：控制参数生效取值（用户提供值 > 规格 `default`）命中触发值 → 必选；
    未命中 → 可缺失，并报**提示级** `ConditionalSkipped`（与 `Missing` 严格区分——
    混用会让"缺参"统计虚高）
  - 不可判定（控制参数未提供且无规格默认值）→ **保守判必选**：分支可能被走到，
    宁可多要一个参数，也不能放过缺失
  - 已知边界：控制参数若靠模板内联 `| default(...)` 兜底，取值无法静态求得，同样落到保守判必选

- **`ParamValue` 反序列化接受裸标量**：手写 YAML 里 `options: ["闭口", 8, 12.5]`、
  `values: ["Right"]` 才是自然写法，强制带标签（`{type, value}`）会把配置文件变成机器码。
  两种形式混用亦可；**序列化恒为带标签形式**（无歧义），且标签统一为小写
  （`{type: integer, value: 8}`，与 `ParamKind` 一致），读取时大小写不敏感以兼容历史载荷。
  带标签形式仍做**类型自洽校验**：`{type: integer, value: 8.5}` 报错而不是静默截断成 `8`

- **文件模板的参数规格外部化**（`core/src/manifest.rs` + `cli/src/context.rs`）：目录模板此前
  **恒为空规格**（`vec![]`），于是 `validate` 对 `Z_START=abc` 这类错误直接放行
  （实测"校验通过：无问题"），`min`/`max`/`integer`/`options`/`required_if` 对用户模板全部失效——
  模板头部写着的参数表只是注释。现在两份来源合成规格：
  - **模板头部 `{# PARAMS: #}`** 解析为 `ParamSpec`（`name`/类型/必选性/描述）。
    兼容两种现存写法：`name 必选 描述` 与 `name type required 描述`，可混用；
    必选标记接受 `必选`/`可选`/`条件必选`/`required`/`optional` 及**分支限定词**
    `可选(ES)`/`必选(FS)`（限定词保留到描述里）。类型可省（记为 `ParamKind::Any`）。
    参数表另用 200 行为界（`NAME`/`DESCRIPTION` 仍限前 10 行）——机床模板十余个参数，
    收尾 `#}` 会落到第 10 行之后
  - **清单 `templates.yaml` 的 `params` 稀疏覆盖层**：补头部表达不了的**约束**
    （`min`/`max`/`integer`/`options`/`required_if`/`default`）。`ParamOverride`
    字段全为 `Option`，因此能区分"没写"与"写成默认值"，只覆盖写了的字段；
    拼错字段名直接报错（`deny_unknown_fields`）
  - 参数表里**无法解析的行会告警**（`warning: <模板>: 第 N 行无法解析…`）而非静默跳过——
    静默丢一行等于静默少一条参数约束

- **模板清单与移植模板**：`templates/templates.yaml`；`turning/undercut.j2`（ES/FS 越程槽，
  完整版）、`turning/undercut_es.j2` / `undercut_fs.j2`（单分支精简版）、
  `turning/_undercut_common.j2`（公共起始段片段）、`grooving/circlip_groove.j2`（卡簧槽）、
  `machines/index_g420/dg_cal_ir9.j2`（INDEX G420 的 R 参数表初始化段）

- **模板整合方案文档**：`docs/TEMPLATE_INTEGRATION_PLAN.md` —— 对源项目 NCTool_V3 的
  结构与配置分析、模板四类分级（原子子程序 / 通用参数化 / 计算密集 / 机床专有）、
  可复用资产逐项判定（直接移植 / 改写后移植 / 不可移植）、五阶段落地路线、风险表；
  §4.4 记录实施中实测发现的三个能力缺口

### Fixed

- **规格默认值的类型从不校验**（`core/src/validate.rs`）：`check_value_constraints` 对非数值类型
  在 `as_f64()` 处提前返回，因此 `Number` 规格配 `String` 默认值会一路静默通过，
  直到渲染时把字符串塞进 `nc_fixed` 才炸——而 ARCHITECTURE 文档声称会查类型。
  现在类型不符报 `TypeMismatch`（且不再叠区间/白名单噪声）

- **渲染错误丢失根因**（`src/error.rs`）：嵌套 `{% include %}` 失败时只报外层包装
  `渲染错误: could not render include: error in "turning/_undercut_common.j2" (in turning/undercut_fs.j2:24)`，
  既不知错在子模板哪一行、也不知错的是什么——实测完全无法定位。现在沿
  `std::error::Error::source()` 链把各层描述以 ` ← ` 追加，末段即根本原因

- **无 `detail` 的渲染错误不可诊断**：minijinja 对部分运行期错误（如对字符串取负）不设 `detail`，
  消息退化成 `invalid operation (in uz_dj_x.j2:83)`。现在 `detail` 为空且错误发生在所传源码
  对应模板时，用 `range()` 取出**出错表达式片段**补进消息
  （实测 `invalid operation (in …:83)（出错表达式：-U_A）`）

- **`render` 的校验失败输出自相矛盾**（`cli/src/commands/render.rs`）：整份报告被塞进
  `CliError::message`，而统一错误输出只给**首行**加 `error: ` 前缀——多行报告的首行会被
  当成错误摘要，提示行排在最前时更会输出 `error: 提示 …`。现在报告走 **stderr**
  （stdout 留给 G-code），错误行只给一句"参数校验未通过（详见上方报告）"

- **`{# PARAMS: #}` 的第三种标记写法未被识别**：`undercut.j2` 用 `可选(ES)`/`可选(FS)`
  标注分支专用参数，此前会为每行刷一条解析告警（且每次 CLI 调用都打印）。

- **模板清单与目录递归**（`core/src/manifest.rs` + `cli/src/context.rs`）：模板元数据
  （名称 / 描述 / 可见性 / 输出文件名与后缀 / 分类 / 机床 / 评审状态）从 Rust 源码外部化到
  `templates/templates.yaml`，新增模板只需放文件 + 加清单条目，**不必改代码重编译**。
  目录模板改为**递归发现**（`templates/` 下任意深度的 `*.j2`），以相对路径为模板名
  （如 `turning/undercut.j2`），支持分类子目录与机床方案包。元数据按
  **清单 > 模板头部注释（`{# NAME: #}` 等）> 文件名/目录名**三级回退解析。
  安全性：跳过隐藏项（`.` 开头）、清单文件自身、符号链接逃逸路径（`canonicalize` 后
  必须仍在模板根内）

- **模板可见性与机床方案包过滤**：`templates list` 新增 `--all`（含隐藏模板，以 `·` 标记）
  与 `--machine <id>`（仅暴露该机床的专用模板）。`visible: false` 用于「完整可用但不出现在
  选择列表」的模板（功能模块、机床初始化段）；机床专用模板须声明 `machine`，未选中该机床时不可见。
  JSON 输出补齐 `visible` / `output_filename` / `output_extension` / `machine` / `status` / `source`

- **列表参数类型**（`ParamValue::List` + `ParamKind::List`）：批量工序模板（如遍历
  `z_offsets` 的卡簧槽）需要列表入参，此前只有数值/字符串/布尔四种类型无法表达。
  `--params-file` 的 JSON 数组递归解析（支持嵌套），`ParamKind::List` 只校验"是不是列表"、
  不校验元素类型（元素约定由模板自身表达，避免静态声明与模板漂移）。对象类型仍被拒绝
  （`ParamValue` 是扁平值模型），需要结构时建模为平行列表

- **枚举参数与候选项白名单**（`ParamKind::Choice` + `ParamSpec.options` +
  `IssueKind::NotInOptions`）：源项目变量库里的 `options` 白名单此前只能写在模板注释中，
  非法工艺选项（`U_FX` 闭口/左开口/右开口、`U_Q` 0/8/10/12.5、`tip_model` B4/DM24）
  会一路渲染成与图纸不符的 G-code。现在在校验阶段硬拦：
  - `ParamSpec::options: Option<Vec<ParamValue>>` + `with_options()` builder，
    `None`/空列表 = 不约束，**旧 YAML/JSON 规格无需改动**（`skip_serializing_if` 保证
    未声明时不写出该字段）
  - 白名单对**所有类型**生效而不只 `Choice`：`Number + options` 表达数值枚举，
    `String + options` 表达文本枚举
  - `ParamKind::Choice` 的**类型匹配有意放宽为"任意标量"**——真正的约束是白名单而非类型，
    否则数值枚举（`0 / 8 / 10 / 12.5`）会先被类型检查挡掉、永远走不到白名单比较。
    `List` 不在其列（列表不可能是扁平白名单的成员）
  - 成员比较 `ParamValue::matches_option`：同变体同值；数值/整数**跨变体按数值相等**
    （`Integer(8)` ≡ `Number(8.0)`，避免 CLI/JSON 把 `8` 解析成 `8.0` 造成假拒绝）；
    文本 `"8"` 与数值 `8` 仍是**不同**候选项，不做隐式归一化
  - 非法值报 `Error` 且**不降级、不替换为默认值或首个候选项**——静默改成默认值同样会产出
    错误 G-code，违背"宁可渲染失败"的项目原则
  - 白名单检查拆为独立的 `check_value_options()`，**排在区间/整数检查之前**：
    `check_value_constraints()` 对非数值类型在 `as_f64()` 处提前返回，字符串枚举
    （白名单的主要用途）若塞进去等于永不执行
  - 规格默认值同样过白名单（写错的 `default` 会在渲染前被静默注入，用户提供的合法值反而
    用不上）；类型不匹配时不再叠报白名单错误，避免同一参数刷出噪声
  - `options_display()` 提供候选值的统一可读渲染，保证"报错里列出的候选值"与
    "实际参与比较的候选值"是同一份数据
  - HTTP API 的 `spec_json` 输出 `options` 字段，供前端渲染下拉选择

- **角度制三角函数过滤器**：`sin_d` / `cos_d` / `tan_d` / `asin_d` / `acos_d` / `atan_d`
  （`_d` = degrees）。裸 `sin`/`cos`/`tan` 保持**弧度制**（与 Rust 标准库一致）。
  动机是「度/弧度静默错坐标」风险：`sin(30)` = -0.988（把 30° 当 30 弧度）而
  `sin_d(30)` = 0.5，错误坐标会直接写进 G-code 导致撞刀。迁移 Python/Jinja2 模板时尤其危险
  ——源项目常把 `math.sin(math.radians(x))` 暴露为 `sin`（度制），与本库的 `sin` 同名不同义

- **`nc_signed(N)` 过滤器**：强制正号 + 固定小数位，`21.0` → `+21.000`、`-4.5` → `-4.500`、
  `0.0` → `+0.000`。对应源项目 Jinja2 的 `fmt_coord`（`f"{v:+.3f}"`），用于**增量坐标/
  旋转量**——部分控制器要求显式正号，省略号会被误判为绝对值。`-0.0` 归一到 `+0.000`
  （控制器对负零处理不一致）。不要用它格式化直径/进给等本无符号语义的值

- **`TemplateRegistry::extract_params(name)`**：提取模板的完整参数闭包（穿透
  `{% include %}` / `{% extends %}`）并剔除系统注入变量（`machine`）。
  此前该逻辑是私有方法、仅服务 `validate`，`inspect` 因此只能看到主模板自身的变量

### Changed

- **INDEX G420 机床方案包全部就位**：源项目 `templates/INDEX G420/` 的 **19 个模板**
  （约 2800 行）全部移植到 `templates/machines/index_g420/`，文件名统一小写化，
  并在 `templates.yaml` 中声明 `machine: index_g420` / `output_extension` / `status: unreviewed`。
  含 6 个主程序（`.MPF`）、1 个 R 参数初始化、1 个键槽倒角、4 个精铣、7 个粗铣（`.SPF`）。
  移植手法与验证见 `docs/TEMPLATE_INTEGRATION_PLAN.md` §5.5

- **`inspect` 穿透 `{% include %}`**：原先只列主模板自身的变量，组合模板的参数表会**漏项**
  （拆分越程槽后实测只显示 6 个参数，而实际需要 11 个），用户按表填参会直到渲染才报错。
  现改用 `extract_params`，并在输出末尾提示引用了哪些片段、行列号指向片段文件自身
- **`templates` 目录结构**：按类型分子目录（`general/` `milling/` `turning/` `grooving/`
  `machines/<id>/`），`demo_gcode.j2` 移至 `turning/`
- **文档同步**：`docs/TEMPLATE_WRITING_GUIDE.md` 过滤器章节重写（新增 §3.1 `nc_signed`
  使用边界、§4「角度制 vs 弧度制」、§4.1「字典查表用下标不用 `.get()`」）；
  README / 模板写作指南 / 机床配置指南中的示例模板路径同步为新结构

### Fixed

- **`{% set x = x | default(v) %}` 被判为「必选参数缺失」**（`src/extract.rs`）：
  静态提取器把「模板局部变量的引用」也记进了必选集合，于是紧随其后的
  `{{ x }}`（读的是上一行 set 出的局部量）把已判定的「可选」整体翻回「必选」。
  源项目大量使用这一兜底惯用法，不修则机床模板无法渲染。现只有**外部参数引用**
  参与必选判定；`{% set total = total + x %}` 这类 RHS 自引用仍正确判为必选
  （`Stmt::Set` 的 RHS 先于目标声明求值）
- **模板头部注释不认空白控制标记**（`core/src/manifest.rs`）：`{# NAME: xxx -#}`
  的显示名会带上尾随 `-`（`templates list` 里肉眼可见）。现取值前剥离 `#}` 前的 `-`

- **`turning/undercut.j2` 互斥分支参数报"必选缺失"**：静态变量提取器不看分支条件的
  运行期取值，把 ES 与 FS 两个互斥分支引用的变量全部视为必选——用户只做 ES 型却被迫
  填 4 个 FS 专用参数。解决方案为**拆分模板**（`undercut_es.j2` / `undercut_fs.j2`
  各只含单一分支，公共段 `include` 抽取），完整版 `undercut.j2` 保留供批量场景。
  验证：拆分版在 Right/Left 共 4 种组合下输出 G-code 与完整版**逐字节一致**。
  未采用 `| default(0)` 兜底方案——那会静默产出 `Z0` 错误坐标，与本项目
  「宁可渲染失败也不静默出错」的红线相悖
- **`templates list` / `inspect` 的示例模板路径**：`demo_gcode.j2` 移动后
  E2E 测试与文档中的旧路径一并更新

### 新增的测试

- 模板清单解析（两种 YAML 形式、默认值、未知字段拒绝、反斜杠规范化、头部注释提取、
  三级回退优先级、目录→分类推断）16 项
- 头部注释空白控制容忍 2 项（带 `-#}` / 不带 `-#}`）
- 自赋值兜底的可选性 2 项（`{% set x = x|default(v) %}` 保持可选；
  无兜底的 `{% set x = x + 1 %}` 仍为必选）
- 角度制三角函数 4 项（含「度制与弧度制结果必须可区分」的撞刀回归守护）
- `nc_signed` 5 项（正号/负号/零、负零归一、与 `nc_fixed` 仅差正号、NaN 拒绝、小数位上限）
- `extract_params` 4 项（穿透 include、剔除系统变量、未找到报错、环引用不栈溢出）
- 列表参数解析 2 项（数组→列表、嵌套数组的错误定位）
- **Web UI v2 落地实现**（`ui/index.html` + `cli/ui/index.html`，两份字节一致）：按
  [docs/UI_DESIGN_PROPOSAL.html](docs/UI_DESIGN_PROPOSAL.html) 重构界面 ——
  结果区动作条（复制 / 下载并入结果区，紧邻其作用对象）、机床 chip 移入结果区头部并带
  「已按 &lt;id&gt; 重新生成」反馈、参数区必填进度与分组、**字段级就地校验**、
  可选参数折叠、模板卡挂载命名预设 chips、侧栏「最近使用」、长程序**工序索引**（≥40 行，
  可点击跳转高亮）、结果区状态 pill（可点击跳到校验列表）、命令面板（⌘/Ctrl+K）、
  ⌘/Ctrl+⏎ 立即生成、⌘/Ctrl+S 存为预设、&lt;768px「模板 / 参数 / 结果」分段视图、
  模态改为右侧抽屉（保留上下文）。设计令牌补齐：亮色升级为对等主题、`--text-dim` /
  `--text-faint` 提亮以达正文对比度 ≥ 4.5:1、字号/间距/圆角/动效统一
- **`output/ui-v2-acceptance.py`**：无头 Chrome 验收套件（67 项断言），覆盖全链路、
  就地校验、输出选项、工序索引、机床切换、主题、最近使用、命令面板、抽屉、分段视图
- **`docs/UI_ACCEPTANCE_CHECKLIST.md` §10**：v2 走查记录（演示模式 67/67、服务模式 14/14、
  D-04 逐字节一致 2/2）与新增验收项 N-01…N-12

### Changed（Web UI v2）

- **Web UI 模式自动判定**：`API.mode` 由硬编码 `"server"` 改为按协议判定 ——
  `file://` 打开走演示模式（离线完整可用；此前双击本地文件会因 fetch 失败显示空列表），
  `http(s)://` 走服务模式。`nctool ui` 的行为不变
- **错误信息主次分层**：后端技术口径（如「必选参数缺失（模板引用且无默认值兜底…）（第 1 行第 24 列引用）」）
  的主信息改为可照做的说法（「必填，未填写」），模板行列定位降为次级信息，
  完整原文保留在 `title` 中 —— 信息不丢、主次分明
- **localStorage 访问加保护**：`file://` 与隐私模式下持久化失败不再中断界面初始化
- **E2E 契约断言同步**（`cli/tests/cli.rs::ui_http_contracts_and_frontend_mode`）：
  原先断言页面含硬编码 `mode: "server"`，改为断言按协议判定的两条分支
  （`location.protocol === "file:"` → `? "demo" : "server"`），语义等价且覆盖新行为

### Added（文档与工程化）

- **README 安装章节**：新增环境要求（Rust 1.82+、三平台）与三种安装方式——GitHub Release
  预编译二进制（含三平台产物名）、`cargo install --path cli --locked`、作为库引入
  （`nctool-tpl` / `nctool-core`，另附 git 依赖写法）；新增安装验证步骤与顶部目录导航
- **README 使用示例章节**：5 个端到端示例（内置模板浏览→查参→校验→生成、自定义模板文件、
  参数文件 `--params-file` + `--format json` 脚本集成、目录模板与 `template_dir` 配置、
  Web UI / 库调用），命令与输出均为 `nctool 0.2.2` 实测；补充「目录模板需显式指定
  `--template-dir` 或配置、模板名含 `.j2` 后缀」这一易踩点
- **贡献指南**：README 新增速览（前置阅读、环境搭建、与 CI 一致的质量门、改动同步要求、
  提交与分支约定、问题反馈渠道），完整版见新增的 `docs/CONTRIBUTING.md`
  （仓库结构、344 项测试分布矩阵、golden 刷新与复核流程、对外稳定契约、发版流程、
  Reviewer 检查清单）；新增 `.github/PULL_REQUEST_TEMPLATE.md`（PR 描述含质量门 /
  文档同步 / 工艺安全核对项）
- **文档自检脚本**：`scripts/check_docs_links.py` —— 校验 Markdown 相对链接与 heading 锚点
  是否存在（README、`docs/` 全部文档已通过检查）
- **Web UI 设计方案 v2**（设计交付物，未改动任何代码）：`docs/UI_DESIGN_PROPOSAL.html` ——
  基于现有 `ui/index.html` 通读的 9 项现状诊断、信息架构（模态降级为侧抽屉、机床 chip 移至结果区
  头部、下载/复制并入结果区动作条）、设计系统（亮色对等主题与对比度修正、字号/间距/动效令牌）、
  四档响应式断点、组件规格、状态矩阵与键盘映射、可访问性目标、37 项既有验收的保留映射
  + 12 项新增验收建议、P0–P3 分阶段实施路径
- **高保真可交互原型**：`output/ui-prototype-v2.html`（单文件零依赖，可直接双击打开）——
  已实现就地校验、必填进度、预设回填、机床 chip 联动、工序索引跳转、命令面板 ⌘K、
  亮/暗主题、移动端分段视图；配套 `output/proto-smoke-check.py`（无头 Chrome 冒烟测试，
  51 项断言全过）与桌面/移动端截图

---

### 性能与质量门禁（架构评估 P0 项）

架构评估（`docs/ARCHITECTURE_REVIEW.md`）标出的三项 P0 已修复。
共同点是"同一份工作被反复重做"：注册表每次请求重建、模板每次校验重复解析、
覆盖率门禁形同虚设。

#### Performance

- **模板注册表按目录指纹缓存**（`cli/src/context.rs`）：`build_registry` 此前
  每个 HTTP 请求都执行一遍「canonicalize → 读 `templates.yaml` → 读
  `variables.yaml` → 递归遍历目录 → `read_to_string` 全部模板 → 逐模板解析
  `{# PARAMS: #}` 头部」，成本与模板数成正比。改为按
  「模板目录 + 目录树最新 mtime」缓存并共享（`Rc`）。
  **指纹不可省**：无条件长期缓存会让用户改完模板仍拿到旧注册表，
  渲染出与图纸不符的 G-code——属于本项目零容忍的"静默产出错误程序"。
  指纹取不到（IO 异常）时放弃缓存，宁可重算。
  需要可变注册表的调用方走新增的 `build_registry_fresh`（`render` 注册临时
  文件模板的路径），避免临时模板泄漏进共享缓存。

- **模板静态分析只做一次**（`core/src/registry.rs`）：新增
  `TemplateEntry::analysis()`，惰性缓存「未声明变量 + 模板引用」。
  此前 `extract_params` 解析一次、`validate` 又解析一次（`validate_template`
  内部还会再解析），`include` 闭包里的每个子模板同样重复解析。
  现改为共用一份缓存，`Ast` 借用源码无法自引用存入条目，故缓存的是解析产物。
  `collect_include_closure` 相应改为接收条目而非 AST。
  配套：`TplError` 加 `Clone`（缓存失败态需要留存带行列定位的原始错误）。

- **宽松渲染器惰性构建**（`core/src/registry.rs`）：`render_template_lenient`
  此前**每次调用**都新建 `Renderer` 并把全部模板重新注册、重新编译一遍
  （宽松是建环境时的标志，无法在同一渲染器上切换）。改为惰性构建一次并缓存，
  注册新模板时失效；构建失败原因一并缓存，返回语义不变。

#### Fixed

- **覆盖率门禁恢复为真门禁**（`.github/workflows/ci.yml`）：`coverage` job 的
  `continue-on-error: true` 已移除。该 job 自引入起持续失败，而失败被
  `continue-on-error` 掩盖成"非阻断项"，结果是**覆盖率从未被真正度量**——
  CI 全绿并不代表覆盖达标。安装方式最终改用
  `taiki-e/install-action@cargo-llvm-cov`（预编译二进制）：`cargo install --locked`
  会在 runner 上编译整套依赖并撞上 crates.io 索引漂移，连续两次 exit 101，
  改固定版本后耗时 <1s 即失败（说明压根没进入构建）。另加一步 `cargo llvm-cov --version`
  把"没装上"与"装上了但跑不起来"分开，并把覆盖率数字打进 job summary
  （没有数字的门禁等于没有门禁，后续设阈值也需要先有真实基线）。

#### 新增的测试

- `registry_is_reused_while_directory_is_unchanged`：目录未变必须复用同一份
  注册表（`Rc::ptr_eq`）。
- `registry_is_rebuilt_after_template_edit` / `registry_is_rebuilt_when_template_added`：
  改内容、加文件都必须重建，且新内容真正生效（用旧注册表会渲染出错误的 G-code）。
- `fresh_registry_does_not_pollute_shared_cache`：`build_registry_fresh` 注册的
  临时模板不得泄漏进共享缓存。
- `analysis_is_computed_once_and_matches_direct_extraction`：缓存结论必须与
  直接 `parse` + `extract_undeclared` 逐项一致，且重复取用命中同一份。
- `lenient_renderer_cache_sees_templates_registered_later`：宽松渲染器缓存
  必须随模板注册失效，否则新模板在宽松模式下报 `TemplateNotFound`。

---

### 双输入面一致性与 CI 有效性（架构评估 P1 项）

#### Fixed

- **CI 的 clippy / test / doc 只覆盖了根 crate**（`.github/workflows/ci.yml`）：
  根目录**既是 workspace 根又是一个 package**，而 cargo 在没有 `default-members`
  时默认只选根 package。三条命令都漏了 `--workspace`，于是只对 `nctool-tpl` 生效，
  `nctool-core` / `nctool-cli`（代码主体、绝大多数测试）**从未被 CI 真正检查过**，
  却一直显示全绿——与覆盖率门禁是同一类"假绿"。现补齐 `--workspace`，
  README / CONTRIBUTING / PR 模板里的同款命令一并订正。
  随之暴露并修掉 8 处 rustdoc 告警（`core/src/derive.rs`、`core/src/manifest.rs`：
  指向私有项的链接改为代码 span，`[Q16]` 标记转义）。CI 实测用例数由
  155（仅根 crate）变为 **492**。

- **`--param` 归一规则的 Rust / 前端分歧**（`cli/src/args.rs` + `ui/index.html`
  与 `cli/ui/index.html`）：前端 `coerceParamValue` 与后端 `coerce_param_value`
  有 4 处真实分歧——`01.5`（Rust 取数值、JS 取文本）、`0x10` 与 `Infinity`
  （JS `Number()` 能解析而 Rust `f64::from_str` 不能）、`true` 配数值型规格
  （Rust 取布尔、JS 取文本）。两侧不一致意味着 CLI 与 Web UI 对同一输入会产出
  不同的 G-code，属本项目零容忍的静默错误。现前端补齐布尔字面量推断、
  把数值判定收紧到 Rust `f64::from_str` 的语法（十进制 + 可选指数，且必须有限）、
  白名单比较改为严格同类型（不再出现 `String(true) === "true"` 匹配布尔候选项）。

#### Added

- **`--param` 归一规则对拍门禁**：用例与期望值集中在
  `scripts/param_parity_cases.json`（40 例，覆盖前导零、指数溢出、十六进制、
  混型白名单、布尔与声明类型的优先级），后端 `args.rs` 的
  `param_coercion_matches_shared_fixture` 与前端 `scripts/check_param_parity.mjs`
  **消费同一份**。脚本从 HTML 里抠出真实函数再跑，不在脚本内重写一份
  （重写就等于第三份实现），并额外比对两份 UI 是否互为镜像。
  改任一侧而不同步 fixture，另一侧立刻失败；CI 在 ubuntu 上跑该脚本。

---

### 渲染入口的有限性闸门（架构评估 P1-3）

#### Fixed

- **公开渲染入口不再绕过有限性检查**（`core/src/registry.rs`）：
  `TemplateRegistry::render` / `render_with_machine` / `render_template` /
  `render_template_lenient` 此前完全不走校验，而 NaN/Inf 的拦截只存在于
  `validate` 与管线 `generate*`——直接调用这些入口的调用方（库使用者、
  自定义上下文场景）会拿到含 `"NaN"` / `"inf"` 的输出，机床走到非法坐标。
  现在在 `render_template*` 这一层加了闸门 `ensure_finite_context`：
  深度优先扫描上下文，命中非有限数即返回 `TplError::Render`，报错带路径
  （如 `passes[1].z`）。放在这一层而非各参数入口，是为了**同时覆盖参数集、
  机床系统变量与自定义上下文**；宽松渲染同样不放宽非法数值
  （参数可以缺省，NaN/Inf 不行）。

#### Added

- `nctool_tpl` 再导出 `ValueKind`（`Value::kind()` 的返回类型）：下游判断
  "是数值还是容器"需要它，此前只能靠 `try_iter()` 试探。
  注意它在 `minijinja::value` 下，不在 crate 根。

---

### 本地 UI 的跨站防护与安全响应头（架构评估 P1-4）

#### Added

- **`/api/` 请求的跨站防护**（`cli/src/server.rs::cross_site_guard`）：
  只绑回环并不足以挡住浏览器侧攻击——任意网页都能向 `127.0.0.1:<port>` 发请求
  （DNS rebinding / CSRF）。现在按两个头判定：`Origin` 不在同源白名单内 → 403；
  `Sec-Fetch-Site` 不是 `same-origin` / `none` → 403。
  **两个头都不存在时放行**（curl 等非浏览器客户端）：本服务是命令行工具，
  刻意保留"直接调 API"的用法，而浏览器的 `fetch` / 表单提交**必然**带
  `Origin`，伪造不了。判定放在路由之前，被拒的请求不必再读请求体、不必建注册表。
- **所有响应统一附加安全头**（`SECURITY_HEADERS`）：CSP（含
  `object-src 'none'`、`base-uri 'none'`、`frame-ancestors 'none'`、
  `form-action 'none'`）、`X-Content-Type-Options: nosniff`、
  `Referrer-Policy: no-referrer`。错误响应同样带，避免 403 / 500 成为绕过 CSP 的口子。

#### 已知取舍

- CSP 的 `script-src` / `style-src` 仍保留 `'unsafe-inline'`：前端是单文件内嵌
  页面，脚本与样式都是内联的。它挡不住"页面内被注入 `<script>`"，但能挡住
  外链加载与嵌入。彻底去掉需要给 `<script>` / `<style>` 注入每响应 nonce，
  列为后续加固项。
- 未实现评审建议的"启动一次性 token 拼进 URL"：它要求前端每个请求都带 token
  （要改 HTML 与 JS），而生成不可预测的 token 需要引入随机源依赖——本项目的
  依赖策略是尽量少加传递依赖。现有判定已覆盖同一威胁模型（跨站驱动本地服务），
  故暂缓。

---

### 覆盖率阈值门（架构评估 P0-2 收尾）

#### Changed

- **`coverage` job 由"报数字"变成"报数字 + 卡阈值"**（`.github/workflows/ci.yml`）：
  新增 `--fail-under-lines 89`。引入时实测基线 **90.81% 行覆盖**（502 项测试，
  区域 90.07% / 函数 90.64%）。
  余量是刻意留的：卡在当前值会让"新增少量未覆盖代码"也变红，门禁随即被绕过；
  1.8pt 约等于 165 行新代码，真掉这么多就是覆盖在退化。**覆盖率提升后应上调此值。**
- 阈值与生成**放在同一步**：`cargo llvm-cov report` 不接受 `--all-features`
  （只读已有数据），拆两步会引入"读回的数据与生成时不一致"的风险。
- `Coverage summary` 与 `Upload coverage artifact` 加 `if: always()`：
  门禁失败时摘要与 lcov 产物**仍要产出**——排查"覆盖为什么掉下去"正需要它们。

#### 已知覆盖洼地（下一步补测的优先级）

`cli/src/server.rs` 74.01%、`src/extract.rs` 80.53%、`cli/src/context.rs` 88.09%
是当前拉低总体的主要来源（`cli/src/output.rs` 已于同日补测，见下）。

---

### 补测 CLI 输出层

#### Changed

- **`cli/src/output.rs` 由 0 测试补到 14 项**：行覆盖 **65.22% → 98.41%**、
  函数覆盖 **61.43% → 100%**（此前是全项目最低）。该文件承载两项**对外契约**
  ——退出码矩阵（README 有完整表）与 `--format json` 的包络形状，
  此前只被 `cli_e2e` 间接覆盖到一部分。
- 为可测性把三段逻辑提成纯函数（行为不变）：`text_ok_buf`（保证恰好一个结尾
  换行）、`json_error_text`（`silent` 时返回 `None`）、`json_ok_text`；
  `print_error` / `print_ok` 退化为"构造文本 + 写出"。
- 覆盖率阈值随之由 **89% 上调到 90%**（基线 91.30%，余量 1.3pt ≈ 120 行）。

#### Added

- 退出码矩阵逐项钉住（13 个分类）+ 未知分类兜底归 1；
- 四个 `From` 转换的分类映射：`io::Error`、`RegistryError`（5 个变体）、
  `PipelineError`（5 个变体，含兜底 `pipeline`）、`TplError`；
- `silent` 语义：JSON 通道**一条都不发**（消费方不该收到两条错误对象）；
- JSON 包络形状（`ok` / `error.kind` / `error.message` / `data`）与结尾换行；
- `text_ok_buf` 边界：已带换行时不得再补（否则 `$(nctool ...)` 会多出空行）。

---

### extract 的属性测试（架构评估 P1-2 补完）

#### Added

- **`tests/extract_invariant.rs`：可选判定的单侧不变量**（300 个生成用例）。
  断言"被判为可选的变量，缺省后严格模式必须渲染成功"——判错就意味着用户拿到
  渲染失败、却查不出缺哪个参数（`extract.rs` 修过的真实缺陷正是这一类）。
  反方向（判为必选 ⇒ 缺省必然失败）**刻意不断言**：本项目"宁多勿漏"，
  `{% if a is defined %}{{ a }}{% endif %}` 中 a 缺省时其实能渲染，仍记为必选。
- 用例由确定性 LCG 生成（**零依赖**，不引入 proptest：失败可复现，也不给依赖树
  加负担）；片段库覆盖兜底 / `defined` / `set` 惯用法 / `with` / `for` / `macro` /
  注释 / 无变量模板等形态，并带**生成器退化保护**：样本必须同时出现"无未声明
  变量""含可选""含必选"三种形态，否则片段库被改坏后测试会退化成空断言。
  该保护首次运行即生效（当时片段库全含变量，被拦下后补了无变量片段）。
  已验证测试真有牙齿：临时把提取结果全判为可选 → case 0 立刻失败并打印模板。

---

### 校验核心拆分（架构评估 P2-1）

#### Changed

- **`check_vars` 由 245 行拆为 55 行编排 + 7 个专职函数**（`core/src/validate.rs`）：
  `check_derive_shadowed`（20）/ `check_spec_defaults`（24）/
  `check_spec_declarations`（44）/ `check_var_values`（41）/ `check_finite`（20）/
  `check_missing`（57）/ `check_unused`（24）。
  纯重构，**行为零变更**——由 `validate.rs` 现有的 62 项测试守住。
  拆分原则：派生那段只能留在编排函数里（派生集合要么新建、要么退回入参，
  返回的引用可能指向二者之一，借用关系无法封装进函数返回值），已在代码里注明；
  白名单与区间/整数本来就有专职函数（`check_value_options` /
  `check_value_constraints`），故不再往下拆，只保留"类型不匹配即短路"的顺序逻辑。
  另：`referenced`（模板引用集合）原先在两处各建一次，现由编排函数算一次传下去。

---

## [nctool-core 0.2.2] - 2026-09-10

「facing 面铣模板 + Web UI 真实浏览器验收修复 + 三平台 Release 准备」（CLI 配套改动见 [nctool-cli 0.2.2]；`nctool-tpl` 本轮无改动，保持 0.3.2）

### Added

- **`facing` 面铣模板**（Backlog「内置模板库扩充」首项）：矩形区域 zigzag 往复行切，
  参数 `x0/y0/length/width/depth/feed`（必选）+ `stepover/safe_z/plunge_feed`（可选，
  `stepover` 默认 10、`plunge_feed` 缺省取切削进给）；行数 = ceil(width/stepover)，
  步进与切削分块（非同块斜线），三机床 golden 基线 + 工艺核对清单 §6 同步（18 组
  基线，原 15 组）
- **`.github/workflows/release.yml` 三平台二进制 job**（F1.3）：tag 推送时构建
  Windows / Linux / macOS（arm64）release 二进制并上传为 GitHub Release assets
  （文件名带 target 后缀，`x86_64-pc-windows-msvc` / `x86_64-unknown-linux-gnu` /
  `aarch64-apple-darwin`）；crates.io publish 流程保持不变

### Fixed

- **参数规格必选/默认值解析**（server 模式）：API 返回 `default: null`（必选参数）
  与带标签默认值 `{type,value}`（可选参数），前端此前用 `p.default === undefined`
  判断，导致必选参数被误判为可选（表单显示「必选 0」）、默认值对象被直填入输入框
  （`[object Object]`）。新增 `paramDefault()` 统一取值，必选判定/默认预填/
  placeholder/侧栏统计全部走该函数（`ui/index.html` 与 `cli/ui/index.html` 字节一致）
- **数据源文本**：server 模式下页脚不再停留在静态「演示数据（本地渲染）」，
  成功/失败分别显示「数据源：服务端 API」/「（加载失败）」
- **移动端输入框字号**：`@media (max-width: 480px)` 的 16px 规则被更高特异性的
  `.param-input-wrap input[type=...]` 13px 规则覆盖，iOS Safari 聚焦时仍会放大整页；
  在媒体查询内追加同特异性后定义规则，实测 375px 视口输入框渲染 16px

### 验收

- **Web UI 人工验收 37/37 通过**（`docs/UI_ACCEPTANCE_CHECKLIST.md`，E1.2 + D4.3）：
  §1 全链路 8/8、§2 移动端 11/11（375px iframe 模拟视口）、§3 校验定位 5/5、
  §4 前后端逐字节一致 5/5、§5 机床切换 4/4（含自定义机床 hero_x9）、§6 主题 2/2、
  §7 已知 UX 2/2。清单按实测修订 4 处与实现不符的描述（D-01-01 参数口径、
  D-03-03 类型不匹配 UI 层不可达、D-03-05 校验内联于 `/api/render`、
  D4.3-04 CLI 与 UI 读同一层叠配置）

### 文档

- `docs/MACHINE_CONFIG_GUIDE.md`：明确自定义机床须提供模板引用的全部 `machine.xxx`
  键（缺失键严格模式渲染失败，属防御行为），并建议以 generic 为基线复制
- `docs/PROCESS_CHECKLIST.md`：新增 §6 facing 工艺核对（含 golden 输出逐行说明），
  原 §7/§8/§9 顺延

---

## [nctool-tpl 0.3.2] - 2026-09-03

"第三轮彻底代码审查修复"（核心/CLI 配套改动见 [nctool-core 0.2.1] / [nctool-cli 0.2.1]）。

### Fixed

- **`{% from %}` 别名语义修复**：`{% from "m" import a as b %}` 此前把导入名与别名的
  角色弄反——别名被误报为外部必选参数（上层校验要求用户提供根本不需要的参数）、
  导入名被误判为模板局部（漏报真实引用）。现对齐 minijinja 绑定语义（有别名绑定
  别名，无别名绑定导入名），两者均不再进入未声明集合
- **`{% block %}` 作用域修复**：块体按 minijinja VM 帧语义视为独立作用域，块内
  `set` 不再外泄。修复"块内 `set` + 块外引用"时校验放行、宽松模式静默输出不完整
  G-code（如 `G1 F`）的漏报
- **未定义变量名恢复防错位**：include/extends 期间子模板报错时，不再用主模板源码
  恢复变量名（子模板字节范围套在主模板上会得到同偏移处的无关标识符）；仅当错误
  确属所传源码对应模板时才恢复
- **`Parse.col` 列口径统一为字符**（与 `Variable.col` 的 minijinja span 口径一致）：
  错误行内含多字节字符（如中文注释）时列号不再虚高
- **`set_path_loader` 安全加固**：模板名自加校验，拒绝空名、Windows 盘符前缀
  （`C:`——`PathBuf::push` 会整体替换 base，可逃出根目录）、绝对路径；安全文档
  改写为与引擎实际行为一致（`..`/`.`/`\` 段本就被 minijinja `safe_join` 拦截，
  原文档示例方向反了）
- `nc_pad` 超范围值（如 `1e300`）不再饱和截断为 `i64::MAX`，直接报错
- `nc_fixed(小数位)` / `nc_pad(宽度)` 设上限（32 / 1024），消除巨量分配导致的进程 abort

### Changed

- **模块拆分**：`lib.rs`（约 2500 行 → 文档 + 再导出 + 测试）拆为
  `error.rs` / `extract.rs` / `filters.rs` / `renderer.rs` 四个模块，公共 API 不变
- `add_template` 同名静默替换语义写入文档
- 文档明确有限性防线范围：仅覆盖本库注册的过滤器，裸 `{{ x }}` 与内建操作不保护
- 新增 `extract_template_refs`：提取 `{% include %}`/`{% extends %}`/
  `{% import %}`/`{% from %}` 的静态（字符串字面量）模板引用名，供上层递归校验
  组合模板

---

## [nctool-core 0.2.1] - 2026-09-03

"阶段 A（需求与设计收口）落地 + 第三轮彻底代码审查修复"：golden 基线 15 组、
MachineConfig 键名 schema、API 冻结清单明确。

### Added（阶段 A）

- **golden 基线扩充到 15 组**（A2）：5 内置模板 × 3 机床预设，每组冻结渲染输出
  （`.nc`）与校验报告（`.report.txt`），取代原有 4 文件弱覆盖；`assert_golden` 支持
  `NCTOOL_UPDATE_GOLDEN=1` 人工刷新（防误改基线掩盖回归）；新增防漏项测试
  （矩阵必须覆盖全部内置模板）
- **MachineConfig 键名 schema**（A4）：`KNOWN_CONFIG_KEYS` 登记 20 个已知键
  （类型/默认值/描述）+ `validate_config_keys`；未知键 / 非整数的
  `program_digits` 类键值 / 非枚举的 `units` 类键值给出结构化告警。
  generic 预设与 schema 默认值一致性测试，防"模板用不到、改配置不报错"的静默漂移

### Fixed（第三轮审查）

- **内置模板 `program_header` 修复（P1）**：`G{{ machine.coordinate_system }}` 与
  配置值 `G54`/`G94` 叠加产出非法双前缀 `GG54`/`GG94`；改为直接输出配置值。
  原三处 `contains("G54")` 弱断言被 `"GG54"` 穿透，现全部升级为字节级 golden
- **内置模板 `tool_change` 修复（P1）**：刀具号裸输出 f64（`M6 T5.0`，T 字址标准
  只接受整数）；改为 `T{{ tool_num | nc_strip }}`，并补 tool_change/safe_move 的
  字节级渲染测试（原零覆盖）
- **内置模板 `drill_cycle` 规格去重（A1 核对随手修复）**：`r_plane` 规格重复链
  调用 `with_min(0.0).with_unit("mm")`，收敛为单份
- **校验穿透 include/extends（P2）**：组合模板引用的已注册模板，其必选参数同样
  参与渲染前校验（环引用防护）；此前主模板 include 子模板时，子模板必选参数
  缺失只能在渲染阶段才暴露，违背"渲染前可发现错误"的设计承诺
- **`registry.render` 与 `validate` 口径对齐（P2）**：render 应用规格默认值兜底；
  新增 `render_with_machine`（注入机床系统变量 + 兜底）。消除文档推荐流程
  "validate 通过 → render 失败"的分歧
- **新增 `GCodeGenerator::generate_lenient`（P1，配合 CLI）**：宽松生成复用严格
  管线的规格默认值兜底、`machine` 注入与后处理，保证宽松输出是严格输出的超集
- **错误链完整化（P2）**：`PipelineError`/`RegistryError` 标记 `#[non_exhaustive]`
  并实现 `source()`；管线不再 `map_err(|_|…)` 吞掉注册表错误（新增
  `PipelineError::Registry` 变体）；`RegistryError::Compile` 结构化携带
  `(name, TplError)`，Display 不再三层重复模板名
- **去重**：`apply_spec_defaults`/`build_render_context`/`param_to_minijinja` 收敛
  到 model 模块单份实现（原先 core 内两份 + CLI 一份共三处拷贝）
- `GenerationOptions` 行号生成修复：`step=0` 视为 1（此前产出全 `N0000`）；加法改
  `checked_add` 防溢出
- 删除死公共 API：自由函数 `has_errors`、类型别名 `ValidationResult`（零调用）
- `ParamSpec` 文档修正：`default` 与 `required` 非互斥（与既有用法/测试一致）
- 校验错误消息携带模板名 + 行列定位（此前只报参数名）
- 参数名与系统注入变量（`machine` 等）同名时输出警告（此前被静默覆盖）
- **机床配置键接线**：`program_prefix`/`program_digits` 接入 program_header，
  `units` 替代硬编码 metric；`line_number_prefix`/`line_number_digits` 接入
  G-code 后处理行号（此前均为装饰性键）
- `registry.render`/`render_with_machine` 文档标注绕过校验层的 NaN 风险；
  `apply_spec_defaults` 文档写明优先级（用户值 > 规格默认 > 模板内联 default）
- 声明 `rust-version = "1.82"`（三个 crate）

---

## [nctool-cli 0.2.1] - 2026-09-03

"阶段 A 接线 + 第三轮彻底代码审查修复"。

### Added（阶段 A）

- `machine show` 输出配置告警（A4 接线）：未知键 / 非法值经
  `validate_config_keys` 检查后以 `⚠` 行展示（text）并附 `warnings` 数组（JSON），
  不阻断命令成功——自定义机床配置携带扩展键仍可正常使用

### Fixed（第三轮审查）

- **`render --lenient` 收编核心管线（P1）**：此前宽松路径绕过规格默认值兜底与
  全部后处理——旗舰模板 `drill_cycle` 省略 `r_plane` + `--lenient` 反而渲染失败
  （宽松比严格更易失败），`--line-numbers/--header/--ascii/--strip-blank` 在该分支
  静默失效。现改走 `GCodeGenerator::generate_lenient`，并删除 CLI 侧
  `build_context`/`render_lenient` 副本（上下文构建收归 core 单份实现）
- **`--out` 写入安全（P2）**：拒绝输出路径与源模板相同（防止渲染结果覆盖并销毁
  模板源码）；父目录缺失时自动创建（对齐 `templates new` 的目录策略）
- **配置解析不再 gate 全部命令（P2）**：`completion`/`ui`/`part` 不读配置文件，
  CWD 存在损坏的 `nctool.toml` 时补全生成不再一并失败
- **脚手架修复**：`templates new` 骨架的坐标系行删除多余 `G` 前缀（与
  program_header 同源的 `GG54` 问题）
- `--lenient` 帮助文案对齐实际语义（经过滤器引用的变量仍需具体值才能求值）

### Changed

- **退出码矩阵**：0 成功 / 1 校验失败 / 2 参数错误（与 clap 一致）/ 3 IO / 4 配置 /
  5 模板·机床未找到 / 6 渲染失败（此前一律 1）
- `validate --format json` 失败输出补齐 `error:{kind,message}`（此前仅 `data`）
- `--param` 类型强制后缀 `k:s=k:n=k:b=`；前导零纯数字（`007`）保持字符串；
  `--help` 写明推断规则
- `--params-file` 数值走 `is_finite` 防护（`1e999` → inf 拒绝），与 `--param` 对齐
- 全局配置路径遵循平台约定（Windows `%APPDATA%\nctool`、Unix `XDG_CONFIG_HOME`），
  兼容回退 `~/.config`；项目配置从 CWD **向上递归**查找
- `config show` 展示全局/项目配置来源路径；配置单次加载并缓存于 `Ctx`
  （`machine list`/`resolve_machine`/`config show` 不再重复读盘）
- `machine list` JSON 增加 `builtin` 标记（text 通道原有"自定义"标注对齐）
- `render` JSON 成功输出附 `warnings` 数组
- `templates show`/`inspect` 模板解析优先级与 `render`/`validate` 统一（注册表名 → 文件路径）
- stdout 断管道（`| head`）不再 panic；`templates new a.j2` 不再生成 `a.j2.j2`；
  `--dir`/`--param` 帮助文案对齐实际行为
- CI 增加 `windows-latest`（此前仅 ubuntu，平台特有路径/目录行为无覆盖）

### 测试

- workspace 总计 **279 项**（2026-09-03 实测，全绿）：nctool-tpl 104 单元 + 18 集成 + 1 文档、
  nctool-core 84 单元 + 10 集成、nctool-cli 23 单元 + 39 集成。
  相对 nctool-cli 0.2.0 基线 211 项 +68（阶段 A 新增：core +4 单元 +1 集成、
  cli +1 集成——合计 +6，余为第三轮审查补测）

---

## [nctool-cli 0.2.2] - 2026-09-10

> 本段原为第二处 `[未发布]` 标题，但其中全部内容（Web UI 响应式 D5、CLI E2E 契约
> E1.1、golden 换行 E2.4、发版准备 F、UI 体验修正、A3 冻结清单、A5 文档基线）
> 均已包含在 tag `nctool-cli-v0.2.2`（提交 `9e6917a`）中 —— 发布时漏改标题，
> 导致 [nctool-core 0.2.2] 段的「CLI 配套改动见 [nctool-cli 0.2.2]」指向不存在的
> 段落。2026-09-18 补齐标题与本节说明。

### Added（阶段 D5 — Web UI 响应式）

- **移动端断点（≤480px）**：`ui/index.html` 新增 `@media (max-width: 480px)`，
  覆盖输入框 16px 字号（规避 iOS Safari 聚焦自动放大）、触摸目标 ≥38px、顶栏压缩、
  侧栏限高 220px（保证预览区不被压没）、参数行换行、预览工具条换行、
  选项卡横向滚动、关于/帮助键值列表单列。亮/暗主题为既有能力，本次仅补响应式

### Added（阶段 E1.1 — CLI E2E 契约测试）

- 新增 `cli/tests/cli_e2e.rs`：44 用例覆盖全部 **10** 个顶层子命令
  （`templates` / `inspect` / `validate` / `render` / `generate` / `machine` /
  `config` / `ui` / `part` / `completion`）的正常与异常路径，逐条断言退出码 0–7。
  `ui` 为阻塞服务，仅覆盖启动前的回环守卫以免测试挂起。
  该文件是**契约测试**：退出码或错误分类的任何变更都必须在此同步
- 退出码矩阵以表形式固化在测试文件头注释中，与 `CliError::exit_code` 一一对应

### Changed（阶段 E1.1 — 退出码契约修正）

- **`templates new` 重名：退出码 3 → 6**。错误分类由 `io` 改为 `template_duplicate`。
  重名是业务冲突而非 IO 失败，且与 `RegistryError::Duplicate` 的分类保持一致
  （`template_duplicate` 此前在退出码矩阵中已定义却无使用点，属死分支）。
  ⚠️ 破坏性变更：脚本若依赖旧码 3，需改为 6
- **`completion` 新增 `powershell` / `pwsh` 别名**：clap 的 kebab-case 默认名为
  `power-shell`，不符合用户习惯；三种写法现等价（仅新增，不移除原值）

### Added（阶段 E2.4 / E4）

- **golden 换行符归一化（E2.4）**：`core/tests/integration.rs` 的 `assert_golden`
  在比较与 `NCTOOL_UPDATE_GOLDEN=1` 刷新两侧均过 `normalize_newlines`，
  golden 基线恒为 LF，断言不再受平台 / git 检出配置影响。
  新增 `golden_files_are_lf_only` 用例在落盘层把住关口（B4.2 基线保护）
- **万行级实测（E4.2）**：新增 `core/tests/large_program.rs`，覆盖
  行号上限截断、恶意位宽夹紧、`step=0` 兜底，以及两条 `#[ignore]` 的万行实测
  （`cargo test --release -- --ignored --nocapture`）。
  实测：10000 行带行号 **1.77 ms / 203 KB**；`line_number_digits=1e9`
  被夹到 32，输出 **231 KB / 1.81 ms**，无内存放大
- **后处理性能基线（E4.1）**：新增 `core/benches/pipeline.rs`，补齐原基准缺失的
  后处理项（`cargo bench -p nctool-core --bench pipeline`）

### Added（阶段 F 阶段发版准备）

- **F1.4 文档同步**：`README.md` 补 `generate` / `ui` 子命令示例 + **退出码矩阵表**
  （0–7 全档 + 典型触发）；补管线后处理性能基线与万行实测；
  `core/README.md` 补 ASCII 清洗、修正过时的测试计数 63+8 → 84+11+3
- **F2.1《机床配置指南》**：`docs/MACHINE_CONFIG_GUIDE.md`——`MachineConfig` 模型、
  3 套内建预设（`generic` / `wfl_m65` / `index_ms40`）、`KNOWN_CONFIG_KEYS` 完整清单
  （6 组分类）、`nctool.toml` 自定义、加载顺序、关键约束（`line_number_digits`
  夹紧到 32 的内存安全理由、`Choice` 键合法性、未知键告警等）
- **F2.2《模板编写指南》**：`docs/TEMPLATE_WRITING_GUIDE.md`——必选/可选启发式、
  NC/数学过滤器、机床配置引用、多模板 include/extends、validate/render 校验分层、
  调试技巧、反模式表（`(x/2) | default` 兜不住、`a.b | default` 同理、模板里手写
  `N0010` 双重编号等）、发布前 checklist
- **F3.1 Issues 模板**：`.github/ISSUE_TEMPLATE/` 三件套——`bug_report.yml` 七段
  （版本/组件/OS/复现命令/模板源码/期望/实际/logs）、`feature_request.yml` 五段
  （动机/提案/替代/影响面/兼容）、`config.yml` 关闭空 issue + 文档/讨论/工艺安全
  三组 contact_links（工艺问题导向 `PROCESS_CHECKLIST` 而非泛用 issue）
- **F3.2 迭代节奏约定**：`docs/RELEASE.md`——版本号策略（0.x 可破坏 / 1.0+ 严格 semver）、
  发布节奏（按需，CI/audit 必须 0 告警）、提交流程（commit 自包含、message 含动机）、
  兼容性窗口（弃用保留 1 minor）、决策与争议（轻量共识 / 7 天讨论期）、
  Backlog 加权打分法（影响×3 + 投入产出×2 + 风险×2 + 依赖×1）
- **F4 Backlog 排序**：ROADMAP 按上述加权法对 5 个候选打分：
  内置模板库扩充 37 / 零件级批量生成 34 / 参数预设 34 / 浏览器内模板编辑 21 /
  i18n 20。明确为「初始基线」，外部反馈后重打分

### Changed（阶段 UI 体验修正）

- **ASCII 清洗复选框副标题歧义修复（用户报告）**：原副标题
  `非 ASCII → ?` 读起来像功能说明而非后果警告，用户勾上后看到
  中文被替换为 `?` 误以为显示 bug。改为 `开启后非 ASCII → ?`，
  并在预览工具栏加红字提示 `⚠ ASCII 模式：非 ASCII 已替换为 ?`，
  下次再触发时一眼能看出原因。详见 `ui/index.html` → `cli/ui/index.html`

### Changed（阶段 A3 — 1.0 API 冻结清单）

**自 `nctool-tpl 0.3.2` / `nctool-core 0.2.1` 起，以下公共 API 面在 1.0.0 前不再
发生破坏性变更**；新增能力走 `additive`：只加不删、不改签名、不改语义。确需
破坏性变更时须先拆出预发布版本并在此节登记。

- **nctool-tpl**：`parse` / `Ast` / `Variable` / `extract_variables` /
  `extract_undeclared` / `extract_template_refs` / `Renderer`
  （`new` / `with_lenient` / `with_strict` / `is_lenient` / `render` / `add_template` /
  `set_path_loader` / `render_template`）/ `TplError`（`#[non_exhaustive]`，变体集冻结）/
  `Value` 再导出 / NC 过滤器（`nc_fixed` / `nc_strip` / `nc_pad`）与数学过滤器
- **nctool-core**：`model`（`ParamValue` / `ParamKind` / `ParamSpec` / `ParameterSet` /
  `MachineConfig`）、`validate`（`spec` / `validate_template` / `validate_with_vars` /
  `ValidationReport` / `ValidationIssue` / `ValidationLevel` / `IssueKind`）、
  `registry`（`TemplateCategory` / `TemplateSource` / `TemplateEntry` /
  `TemplateRegistry` / `RegistryError`）、`machine`（`MachinePreset` / `MachineId` /
  `KNOWN_CONFIG_KEYS` / `validate_config_keys`）、`pipeline`（`GCodeGenerator` /
  `GenerationOptions` / `OutputFormat` / `PipelineError`）

**1.0 前候选破坏性变更（未决，不进 0.3.2/0.2.1）**：

- `safe_move` / `drill_cycle` 的参数规格上界可能按机床动态化（PROCESS_CHECKLIST F1）
- 删除 `AST` 内部结构字段的二次暴露（如有）

### Changed（阶段 A5 — 文档与基线）

- 新增 [docs/PROCESS_CHECKLIST.md](docs/PROCESS_CHECKLIST.md)：工艺核对清单
  （逐行核对结论 + F1–F5 发现项 + 外部评审待办）
- README 顶部新增定位与风险声明（未经工艺验证，投产前必须自行核对）
- `docs/ROADMAP.md` / `docs/DEV_PLAN_CLI_UI.md` 数据校准

---

## [nctool-cli 0.2.0] - 2026-09-01

"阶段 1 CLI 核心能力"：脚本化生成 G-code 全流程可用；同时承载阶段 0 的命令树骨架与工程基线。

### Added

- **完整命令树**：`templates list/show/new`、`inspect`、`validate`、`render/generate`、`machine list/show`、`config init/show`、`completion`（`ui`/`part` 为阶段 2/4 占位）
- **参数输入**：`--param k=v`（类型自动推断：数值/字符串/布尔）+ `--params-file`（JSON 对象，显式 `--param` 覆盖文件）
- **配置层叠**：全局 `~/.config/nctool/config.toml` + 项目 `./nctool.toml`，项目覆盖全局（模板目录/默认机床/自定义机床表）
- **统一错误输出**：`--format text|json` 双通道；JSON 输出结构化错误对象 `{ok, error:{kind, message}}`
- **宽松模式**：`render --lenient` 未定义变量渲染为空（缺失参数不阻断）
- **模板脚手架**：`templates new` 生成带参数规格注释的骨架
- **golden 测试**：CLI 渲染输出与 `nctool-core` 管线逐字节一致

### Changed

- 新增 `cli/` crate 并接入 workspace（`members = [".", "core", "cli"]`），binary 名 `nctool`
- `release.yml` 增加 `nctool-cli-v*` tag 发布通道（对齐 tpl/core 命名约定）

### 测试

- nctool-cli：19 单元 + 29 集成（含 golden / 退出码 / JSON / 脚手架 / 配置层叠），workspace 总计 211 项（本发布基线；后续版本见 [nctool-cli 0.2.1]）

---

## [nctool-cli 0.1.0] - 2026-09-01

"阶段 0 脚手架与命令框架"：clap 命令树骨架 + 全局选项 + 工程基线。

### Added

- 新增 `cli/` crate（package `nctool-cli`，binary `nctool`），依赖 `nctool-core` + `nctool-tpl`
- clap derive 命令树；全局选项 `--machine` / `--template-dir` / `--format` / `--verbose`
- 统一错误类型 `CliError`（text/JSON 双输出通道）
- 参数解析单元测试（类型推断 / params-file / 覆盖优先级）
- CI 五道门（fmt / clippy / test / doc / cargo audit）经 workspace 级命令自动覆盖新 crate

---

## [nctool-core 0.2.0] - 2026-08-31

"第二轮全面代码审查修复"：补齐错误定位与 NC 输出健壮性；模板作用域语义修复见 [nctool-tpl 0.3.1]。

### Added

- **`GenerationOptions::ascii_only`**：开启后 G-code 输出中的非 ASCII 字符替换为 `?`
  （含头部注释与模板名）；仅对 `OutputFormat::Gcode` 生效，`Text` 始终原样

### Fixed

- **machine 元信息注入**：`machine.id` / `vendor` / `model` 现可被模板直接引用
  （原先只注入 `config` 键值表）；config 同名键以元信息优先
- **校验错误类型**：`TemplateRegistry::validate` 错误从 `String` 改为 `RegistryError`
  （新增 `NotFound` 变体）；管线中校验失败不再误映射为 `PipelineError::Render`
- **头部注释 ASCII 化**：管线头部注释改为英文 ASCII 文本（许多 CNC 控制器对非 ASCII 敏感）
- **Memory 模板去重存储**：`TemplateSource::Memory` 不再复制一份源码（源码以
  `TemplateEntry::source_text` 为单一权威来源）

### Changed（破坏性，0.x 阶段）

- `TemplateSource::Memory` 从 `Memory(String)` 改为无载荷变体（源码统一读 `source_text`）
- `MachinePreset::to_config` 移除（与 `config()` 重复）
- `TemplateRegistry::validate` 返回 `Result<ValidationReport, RegistryError>`
- core 依赖 `minijinja` 去除多余的 `unstable_machinery` 等 feature 声明（由 nctool-tpl 启用并合并）
- 依赖升级：`nctool-tpl` 0.3.0 → 0.3.1

### 其他

- 补全发布元数据：`nctool-core` 的 `repository` 指向 GitHub 仓库

### 测试

- nctool-core 52 单元 + 8 集成，workspace 总计 163 项

---

## [nctool-tpl 0.3.1] - 2026-08-31

"第二轮全面代码审查修复"：修正变量提取的作用域语义，补齐错误定位与 NC 输出健壮性。

### Fixed

- **set/with 自引用漏报**：`{% set total = total + price %}` 中右侧引用原被误判为模板局部，
  导致必选参数校验漏报、严格渲染才报错。现 RHS 先在外层作用域求值，再绑定目标
- **作用域泄漏**：`for` / `with` / `macro` 现在创建独立作用域（与 Jinja2 语义一致），
  循环/块内 `set` 的名字不再泄漏到块外；`if` 仍不创建作用域
- **UndefinedVariable 恢复变量名**：debug feature 下错误携带字节范围，尽力从源码恢复缺失变量名
  （`{{ missing }}` → `"missing"`）；属性链场景无法判定缺失位置时留空（宁缺毋错）
- **nc_pad 拒绝负数**：负输入会拼出 `O-001` 这类非法 G-code，现报渲染错误

### 其他

- 新增 `.gitattributes`（统一 LF），消除 Windows 下 autocrlf 幻影改动
- `set_path_loader` 文档补充路径穿越安全性说明（模板视为可信输入）
- 补全发布元数据：`nctool-tpl` 的 `repository` 指向 GitHub 仓库

### 测试

- nctool-tpl 85 单元 + 17 集成 + 1 文档（workspace 总计 163 项，含 nctool-core）

---

## [0.1.0] nctool-core - 2026-08-31

"阶段 1 核心库"：workspace 新增子 crate `core/`（nctool-core v0.1.0），在 nctool-tpl 之上提供面向 G-code 生成的生产级能力，并通过阶段性代码审查。

### Added

- **workspace 化**：根 Cargo.toml 改为 workspace（`members = [".", "core"]`，resolver=2）
- **数据模型**（`model`）：`ParamValue`（Number/String/Bool，serde 带标签）、`ParamKind`、`ParamSpec`、`ParameterSet`（BTreeMap + fluent API）、`MachineConfig`、`Tool`、`Operation`、`Part`
- **参数校验引擎**（`validate`）：`validate_template` / `validate_with_vars` 渲染前校验（必选齐全 / 类型匹配 / 默认值兜底 / 冗余警告），`ValidationReport` 结构化错误，系统变量（默认 `machine`）校验豁免
- **模板注册表**（`registry`）：内存 / 文件 / 内置模板统一管理，内置 5 个 G-code 子程序（program_header / program_footer / tool_change / safe_move / drill_cycle）
- **机床配置**（`machine`）：`MachinePreset`（Generic / WflM65 / IndexMs40）内建配置，模板经 `{{ machine.xxx }}` 引用实现一套模板适配多机床
- **生成管线**（`pipeline`）：`GCodeGenerator.generate`（校验 → 规格默认值兜底 → 上下文合并 → 渲染 → 后处理），`PipelineError` 三变体
- **根导出补全**：`lib.rs` 增加 `validate_template` / `validate_with_vars` / `spec` 根路径导出

### Fixed（阶段性代码审查）

- **非有限数防护**：数值参数为 NaN/Inf 时校验阶段拒绝（原实现会经 JSON 中间层**静默转 0** 污染坐标）
- **上下文构建**：改为 minijinja 原生构造，移除 serde_json 中间层，杜绝非有限数被篡改
- **Text 格式契约**：Text 格式仅渲染、保留原始行（原实现会对所有行 trim，破坏空白）
- **行号逻辑**：`max_line_number` 概念清晰化（原实现将"行数上限"与"行号数值上限"混用）
- **校验去重**：`validate_template` / `validate_with_vars` 提取共享核心 `check_vars`
- **错误映射**：`RegistryError` 新增 `Compile` 变体（原将模板编译失败误映射为 `Io`）；内置模板注册失败不再静默吞错（`expect`）
- **占位测试清理**：移除纯为消告警的 `param_kind_reuse_in_tests`

### 测试

- nctool-core：49 → 55 项（47 单元 + 8 集成），workspace 总计 146 项
- 新增覆盖：NaN/Inf 校验拒绝（含 `validate_with_vars` 路径）、有限数通过、NaN 管线拦截、Text 格式保留空白、行号到上限后停止

---

## [0.3.0] - 2026-08-31

"NC 数值格式化"版本：新增 G-code 专用数值格式化过滤器与严格/宽松渲染模式切换。

### Added

- **NC 数值格式化过滤器**：`nc_fixed(N)`（固定小数位）、`nc_strip`（去尾零）、`nc_pad(N)`（前导零填充），用于 G-code 坐标值、程序号、行号的格式化输出
- **严格/宽松模式切换**：`Renderer::with_lenient()` / `with_strict()` / `is_lenient()`。默认严格（未定义变量报错），宽松模式渲染未定义变量为空字符串；`extract_undeclared` 的必选判定不受模式影响
- **非有限数防护**：所有 NC 过滤器对 NaN/Inf 输入报 [`TplError::Render`]，防止非法坐标写入 G-code
- **测试**：8 项 NC 过滤器测试 + 5 项模式切换测试，总测试 78 → 91 项

### 用法示例

```jinja
O{{ prog | nc_pad(4) }}
N{{ line | nc_pad(4) }} G1 X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }} F{{ feed | nc_strip }}
```

```rust
let r = Renderer::new().with_lenient(); // 宽松模式：未定义变量渲染为空
```

### 测试

- 从 78 项增至 91 项（73 单元 + 17 集成 + 1 文档）
- 新增覆盖：nc_fixed / nc_strip / nc_pad 的常规与异常输入；严格/宽松模式切换、模式与参数提取的独立性

### 性能基线（release，~260 字节 G-code 模板）

| 操作 | 耗时（中位数） | 吞吐 |
| --- | --- | --- |
| `parse` | 2.61 µs | 98 MiB/s |
| `extract_undeclared` | 1.97 µs | — |

"基座打磨"版本：多模板能力、错误细分、API 稳定化、工程化全覆盖。

### Added

- **多模板渲染**：`Renderer::add_template`（内存注册）、`Renderer::set_path_loader`（目录加载）、`Renderer::render_template`（渲染已注册模板），支持 `{% include %}` / `{% extends %}` / `{% import %}`
- **错误类型细分**：`TplError` 新增 `TemplateNotFound` / `UndefinedVariable` / `UnknownFilter` / `UnknownTest` 变体，上层可精准处理
- **可选/必选参数区分**：`Variable.optional` 字段，`default`/`d` 过滤器和 `is defined`/`is undefined` 测试标记的变量为可选
- **列号定位**：`TplError::Parse.col` 从恒为 1 的占位值改为真实列号（基于 minijinja `debug` feature 的字节范围换算）
- **性能基准**：`criterion` benchmark（parse / extract_undeclared / render 三场景），`cargo bench` 可运行
- **CI 流水线**：GitHub Actions（fmt / clippy / test / doc / cargo audit），push 和 PR 触发
- **CHANGELOG**：本文件

### Changed

- **[破坏性] API 收窄**：`Ast` 的 `name` / `source` 从 pub 字段改为私有字段 + 访问方法（`name()` / `source()`），减少未来破坏面
- **[破坏性] 错误枚举扩展**：`TplError` 标注 `#[non_exhaustive]`，未来新增变体不破坏下游；match 请保留通配分支
- **minijinja 版本锁定**：从 `^2.24.0` 收窄为 `~2.24.0`（允许补丁，锁定 minor），因依赖 `unstable_machinery` AST API
- **release 配置**：新增 `codegen-units = 1`（配合已有 LTO + strip）
- **文档**：所有公共 API 补全 doc comment，`cargo doc --no-deps` 零警告；新增 `#![warn(missing_docs)]` lint

### Fixed

- 列号定位：`TplError::Parse.col` 不再恒为 1，现在指向解析器停止位置的最佳近似
- 可选参数误报：有 `default()` 兜底的变量不再被列为必选未声明变量

### 测试

- 从 11 项增至 56 项（38 单元 + 17 集成 + 1 文档）
- 新增覆盖：多模板 include/extends/import、错误细分、边界 case（空模板/纯文本/注释/保留名/循环变量/宏参数/嵌套 default/filter 链/复杂表达式）、列号定位、可选/必选区分

### 性能基线（release，~260 字节 G-code 模板）

| 操作 | 耗时（中位数） | 吞吐 |
| --- | --- | --- |
| `parse` | 2.61 µs | 98 MiB/s |
| `extract_undeclared` | 1.97 µs | — |
| `render` | 5.32 µs | 48 MiB/s |

---

## [0.1.0] - 2026-08-30

初始版本：NCtool 模板解析核心。

### Added

- 核心 API：`parse` / `extract_variables` / `extract_undeclared` / `Renderer`
- 数学过滤器集：sin / cos / tan / asin / acos / atan / sqrt / exp / ln / log10 / pow / floor / ceil（含 NaN/Inf 有限性校验）
- Strict 未定义变量策略：缺失变量直接渲染失败
- 11 项测试
- README、MIT 许可、demo 示例、G-code 模板
