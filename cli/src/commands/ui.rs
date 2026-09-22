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
    run_with_serve(args, |srv, actual| server::serve(srv, actual, ctx.clone()))
}

/// [`run`] 的主体，把阻塞的 `serve` 作为参数注入。
///
/// 为了可测：`run` 的直接形式和 `server::serve` 都会阻塞到进程退出，单测无法
/// 跑完；注入后测试可传一个“发完响应就返回”的假实现，从而覆盖
/// `prepare` → 横幅 → `--open` → 移交 serve 这条完整启动序列。
fn run_with_serve<F>(args: &UiArgs, serve: F) -> Result<(), CliError>
where
    F: FnOnce(tiny_http::Server, std::net::SocketAddr) -> Result<(), CliError>,
{
    let (srv, actual) = prepare(args)?;
    // 先绑定再开浏览器：绑定失败（如端口被占用）时不应留下一个指向死页的浏览器
    // 标签页。`--port 0` 由内核分配端口，故 URL 一律用回读到的实际地址。
    eprintln!(
        "nctool ui 已启动 → {}（Ctrl-C 退出）",
        server::browser_url(actual)
    );
    if args.open {
        open_browser(&server::browser_url(actual));
    }
    serve(srv, actual)
}

/// 启动前的非阻塞部分：解析监听地址 → 绑定 → 回读实际地址。
///
/// 抽出来是为了可测：`run` 的尾部是阻塞的 `serve`，无法在单测中跑完；
/// 而「非回环地址拒绝」「端口占用报错」「`--port 0` 回读实际端口」这三条
/// 真正需要覆盖的逻辑都在这一段。
fn prepare(args: &UiArgs) -> Result<(tiny_http::Server, std::net::SocketAddr), CliError> {
    let addr = server::listen_addr(&args.host, args.port)?;
    server::bind(addr)
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
    use super::*;
    use crate::server::{browser_url, listen_addr};

    fn ui_args(host: &str, port: u16) -> UiArgs {
        UiArgs {
            host: host.to_string(),
            port,
            open: false,
        }
    }

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

    /// 覆盖 `prepare` 的成功路径与 `--port 0` 端口回读。
    /// 端口 0 由内核分配，回读到的 `actual` 必须是真实端口（非 0）——
    /// 否则 `--open` 会打开一个指向 `:0` 的死页（P2-24 回归）。
    #[test]
    fn prepare_binds_and_reads_back_actual_port() {
        let (srv, actual) = prepare(&ui_args("127.0.0.1", 0)).expect("应能绑定回环端口");
        assert_ne!(actual.port(), 0, "--port 0 应回读到内核分配的真实端口");
        assert_eq!(actual.ip().to_string(), "127.0.0.1");
        assert!(browser_url(actual).starts_with("http://127.0.0.1:"));
        drop(srv);
    }

    /// 非回环地址必须在 `prepare` 阶段（绑定前）就被拒绝，不得真的监听。
    #[test]
    fn prepare_rejects_non_loopback_before_binding() {
        let err = match prepare(&ui_args("0.0.0.0", 0)) {
            Ok(_) => panic!("非回环地址应被拒绝"),
            Err(e) => e,
        };
        assert_eq!(err.kind, "args");
        assert!(
            err.message.contains("仅允许绑定回环地址"),
            "实际: {}",
            err.message
        );
    }

    /// 端口被占用时 `prepare` 应返回 IO 错误（而非 panic 或静默成功），
    /// 这样 `run` 才能在开浏览器前失败（P2-24 的「先绑定再开浏览器」）。
    #[test]
    fn prepare_reports_error_when_port_is_taken() {
        let (held, actual) = prepare(&ui_args("127.0.0.1", 0)).expect("先占一个端口");
        let taken = actual.port();
        let err = match prepare(&ui_args("127.0.0.1", taken)) {
            Ok(_) => panic!("端口已占用应报错"),
            Err(e) => e,
        };
        assert_eq!(err.kind, "io");
        assert!(err.message.contains("绑定"), "实际: {}", err.message);
        drop(held);
    }

    /// 覆盖 `run` 的完整启动序列（prepare → 横幅 → 开浏览器分支 → 移交 serve）。
    /// 用注入的假 serve 替代阻塞的真实现，从而在单测里跑完。
    #[test]
    fn run_startup_sequence_hands_server_to_serve() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called2 = called.clone();
        let args = ui_args("127.0.0.1", 0);
        run_with_serve(&args, move |srv, actual| {
            assert_ne!(actual.port(), 0, "应回读到真实端口");
            called2.store(true, Ordering::SeqCst);
            drop(srv);
            Ok(())
        })
        .expect("启动序列应成功");
        assert!(called.load(Ordering::SeqCst), "应把服务实例交回 serve");
    }

    /// `--open` 分支：走一次 `open_browser`。它 spawn 系统打开器（失败被 `let _`
    /// 吞掉，不依赖桌面环境存在），此处只断言调用不 panic 且启动序列仍成功。
    #[test]
    fn run_with_open_flag_still_reaches_serve() {
        let mut args = ui_args("127.0.0.1", 0);
        args.open = true;
        run_with_serve(&args, |srv, _| {
            drop(srv);
            Ok(())
        })
        .expect("--open 下启动序列应成功");
    }

    /// `open_browser` 直接调用：不经过 shell，不 panic（无桌面环境时 spawn 失败被忽略）。
    #[test]
    fn open_browser_does_not_panic() {
        open_browser("http://127.0.0.1:0");
    }
}
