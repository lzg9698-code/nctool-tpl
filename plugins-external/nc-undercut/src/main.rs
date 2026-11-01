//! nc-undercut — external NCtool plugin (protocol v2) for ES/FS undercut
//! batch generation, ported from the retired builtin implementation.
//!
//! The binary is launched by the NCtool host with the installed plugin
//! directory as working directory; it reads `plugin.json`, handshakes over
//! newline-delimited JSON-RPC on stdin/stdout and serves the declared actions.
mod calculation;
mod plugin;
mod rpc;
mod trajectory;

use std::sync::Arc;

fn main() {
    let root = std::env::current_dir().expect("plugin working directory");
    let manifest_path = root.join("plugin.json");
    let manifest = rpc::Server::manifest_from(&manifest_path).unwrap_or_else(|e| {
        eprintln!("nc-undercut: cannot load {}: {e}", manifest_path.display());
        std::process::exit(1);
    });
    let state = Arc::new(plugin::plugin(&root));
    let server = rpc::Server::new(manifest, move |action, input, server, request_id| {
        state.invoke(action, input, server, request_id)
    });
    server.run();
    std::process::exit(0);
}
