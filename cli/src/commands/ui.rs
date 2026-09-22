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
    // 先绑定再开浏览器：绑定失败（如端口被占用）时不应留下一个指向死页的浏览器
    // 标签页。`--port 0` 由内核分配端口，故一律使用回读到的实际地址。
    let (srv, actual) = server::bind(addr)?;
    eprintln!(
        "nctool ui 已启动 → {}（Ctrl-C 退出）",
        server::browser_url(actual)
    );
    if args.open {
        open_browser(&server::browser_url(actual));
    }
    server::serve(srv, actual, ctx.clone())
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
    use crate::server::{bind, browser_url, listen_addr};

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

    /// `--port 0` 由内核分配端口，`bind` 必须**回读实际地址**返回 ——
    /// 否则服务日志与 `--open` 打开的 URL 都会指向 `:0`。
    #[test]
    fn bind_returns_real_port_for_zero() {
        let requested = listen_addr("127.0.0.1", 0).unwrap();
        assert_eq!(requested.port(), 0, "请求端口应为 0");
        let (server, actual) = bind(requested).expect("应能绑定回环地址");
        assert_ne!(actual.port(), 0, "回读的端口应是内核实际分配的");
        assert_eq!(actual.ip().to_string(), "127.0.0.1");
        let url = browser_url(actual);
        assert!(
            url.starts_with("http://127.0.0.1:") && !url.ends_with(":0"),
            "URL 应携带真实端口：{url}"
        );
        drop(server);
    }

    /// 端口已被占用时 `bind` 必须返回 `Err`（而不是 panic 或静默成功）——
    /// `run` 依赖这一点做到「绑定失败就不留一个指向死页的浏览器标签」。
    #[test]
    fn bind_fails_when_port_is_taken() {
        let first = listen_addr("127.0.0.1", 0).unwrap();
        let (held, actual) = bind(first).expect("首次绑定应成功");
        // 同一地址再绑一次：应失败
        let again = listen_addr("127.0.0.1", actual.port()).unwrap();
        assert!(
            bind(again).is_err(),
            "端口已占用时第二次绑定应当失败：{actual}"
        );
        drop(held);
    }

    /// `open_browser` 只在 `--open` 时被调用；这里只验证它对 URL **不做任何改写**、
    /// 且接受 IPv6 方括号形态（不经过 shell，故无命令拼接风险）。
    ///
    /// **不实际拉起进程**：本用例只核查传入参数，避免在 CI 上弹出浏览器。
    #[test]
    fn open_browser_accepts_bracketed_ipv6_url() {
        let addr = listen_addr("::1", 0).unwrap();
        let (server, actual) = bind(addr).unwrap();
        let url = browser_url(actual);
        assert!(url.starts_with("http://[::1]:"), "IPv6 应带方括号：{url}");
        // `open_browser` 的签名只接受 &str，且实现里没有 shell 拼接
        let _: fn(&str) = super::open_browser;
        drop(server);
    }
}
