//! 应用状态：**只存纯数据**（全部 `Send + Sync`），供 Tauri 的 `State<'_, AppState>` 使用。
//!
//! `Ctx` 含 `RefCell<…Rc<…>>`，是 `!Send`/`!Sync` —— **绝不放进 State**，
//! 只在命令体内（`spawn_blocking` 的工作线程里）临时构造。

/// 应用全局状态。
#[derive(Clone)]
pub struct AppState {
    /// 已解析的模板目录（显式参数优先于配置；`None` = 仅内置模板）
    pub template_dir: Option<std::path::PathBuf>,
    /// 已解析的默认机床
    pub default_machine: Option<String>,
    /// 一次性加载的层叠配置（只读；`resolve_machine` 查自定义机床要用）
    pub loaded: nctool_cli::config::LoadedConfig,
    /// 配置层降级 warning（透出到状态栏）
    pub warnings: Vec<String>,
}

impl AppState {
    /// 启动时加载：读配置（**不打印 warning**，交由前端呈现）。
    ///
    /// `template_dir` / `default_machine` 显式参数优先于配置值。
    pub fn load(
        template_dir: Option<std::path::PathBuf>,
        default_machine: Option<String>,
    ) -> Result<Self, nctool_cli::output::CliError> {
        let loaded = nctool_cli::config::load()?;
        Ok(Self {
            template_dir: template_dir.or_else(|| loaded.merged.template_dir.clone()),
            default_machine: default_machine.or_else(|| loaded.merged.default_machine.clone()),
            warnings: loaded.warnings.clone(),
            loaded,
        })
    }

    /// 每命令现建一个 `Ctx`（`registry_cache` 为空 → 首个 `build_registry` 即重建注册表）。
    ///
    /// 保留为设计约定的便利构造器（§3.3）。**异步命令路径不用它**：`Ctx` 是 `!Send`，
    /// 且本方法借用 `&self`，无法移进 `spawn_blocking`；命令一律改用 [`Self::snapshot`]
    /// 取纯数据、在工作线程内 `Ctx::for_embedded` 重建（见 `commands/*.rs`）。
    #[allow(dead_code)]
    pub fn ctx(&self) -> nctool_cli::context::Ctx {
        nctool_cli::context::Ctx::for_embedded(
            self.template_dir.clone(),
            self.default_machine.clone(),
            self.loaded.clone(),
        )
    }

    /// 取出可跨线程的纯数据快照（`Ctx` 是 `!Send`，绝不跨 `await`）。
    pub fn snapshot(
        &self,
    ) -> (
        Option<std::path::PathBuf>,
        Option<String>,
        nctool_cli::config::LoadedConfig,
    ) {
        (
            self.template_dir.clone(),
            self.default_machine.clone(),
            self.loaded.clone(),
        )
    }
}
