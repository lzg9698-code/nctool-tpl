# nctool-gui 开发

## 工具链

- Rust **1.89+**（与 workspace MSRV 一致）。
- Node.js **24 LTS**：`.nvmrc` 为 CI 和支持该文件的版本管理器指定默认版本；Windows 的
  nvm-windows 请显式执行 `nvm use 24`。Node 22.12+ 也在 `package.json` 的 `engines` 范围内。
- Tauri CLI 2：`cargo install tauri-cli --version "^2.0.0" --locked`。
- Linux 还需安装 Tauri 的系统 WebKit/GTK 依赖；请按[官方系统前置条件](https://v2.tauri.app/start/prerequisites/)选择对应发行版。

选择 Node 24 的命令：macOS/Linux（nvm）在 `gui/frontend/` 执行 `nvm use`；Windows（nvm-windows）执行 `nvm use 24`。

## 启动开发版

```bash
cd gui/frontend
npm ci
cd ..
cargo tauri dev
```

`tauri.conf.json` 会运行 `npm --prefix frontend run dev`，并将 Vite 固定在 1420 端口；端口占用时会直接报错，避免桌面壳加载错误地址。

## 构建安装包

```bash
cd gui/frontend
npm ci
cd ..
cargo tauri build
```

当前 bundle target 配置为 Windows NSIS 安装包。跨平台 CLI 发布由仓库根目录的 Release workflow 负责；该 workflow 不构建或发布 Tauri GUI。

前端依赖、TypeScript 和生产构建检查：

```bash
npm ci
npm run build
npm audit --audit-level=moderate
```
