//! NCtool 2.0 host: configuration, composition, CLI/HTTP adapters. No domain models.
pub mod composition;
pub mod engineering;
pub mod server;

pub mod history;
pub mod workbench;

pub(crate) fn asset_error(error: nctool_assets::WriteError) -> nctool_plugin_sdk::PluginError {
    nctool_runtime::config::asset_error(error)
}
