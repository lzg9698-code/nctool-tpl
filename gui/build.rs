//! Tauri build script：读取 `tauri.conf.json`、为 exe 编译 Windows Resource
//! （含 `icons/icon.ico` —— Windows 下缺失会直接报错）。

fn main() {
    tauri_build::build()
}
