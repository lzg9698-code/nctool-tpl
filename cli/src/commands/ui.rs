//! `ui` 子命令：启动本地 Web UI（阶段 C 实现）。
//!
//! 行为（ROADMAP C1）：
//! - 内嵌单文件前端（`include_str!`），无外部静态资源
//! - 默认绑定 `127.0.0.1:8787`；非回环地址直接拒绝
//! - `--open` 在确认监听地址安全后用系统默认浏览器打开页面
//! - 只读 API：模板列表 / 详情、变量提取、机床列表（见 [`crate::server`]）

use crate::cli::UiArgs;
use crate::context::Ctx;
use crate::output::CliError;
use crate::server;

/// `nctool ui`：启动本地 Web UI 服务（阻塞运行，Ctrl-C 退出）。
pub fn run(ctx: &Ctx, args: &UiArgs) -> Result<(), CliError> {
    let addr = server::listen_addr(&args.host, args.port)?;
    if args.open {
        open_browser(&server::browser_url(addr));
    }
    server::serve(ctx.clone(), &args.host, args.port)
}

/// 用系统默认浏览器打开 URL（不经过 shell，避免命令拼接）。
fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        // explorer 以 URL 为参数时转交默认浏览器，无需 shell
        let _ = std::process::Command::new("explorer").arg(url).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

#[cfg(test)]
mod tests {
    use crate::server::{browser_url, listen_addr};

    #[test]
    fn loopback_listen_addresses_are_allowed() {
        assert!(listen_addr("127.0.0.1", 8787).is_ok());
        assert!(listen_addr("::1", 8787).is_ok());
    }

    #[test]
    fn non_loopback_or_hostname_is_rejected() {
        assert!(listen_addr("0.0.0.0", 8787).is_err());
        assert!(listen_addr("192.168.1.5", 8787).is_err());
        assert!(listen_addr("localhost", 8787).is_err());
    }

    #[test]
    fn ipv6_browser_url_is_bracketed() {
        let addr = listen_addr("::1", 8787).unwrap();
        assert_eq!(browser_url(addr), "http://[::1]:8787");
    }
}
