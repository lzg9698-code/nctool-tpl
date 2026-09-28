import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 约定：devUrl 端口固定 1420（见 tauri.conf.json），端口被占则失败而非静默换端口。
export default defineConfig({
  plugins: [react()],
  clearScreen: false, // Tauri 约定：别清掉 CLI 日志
  server: {
    port: 1420,
    strictPort: true, // 端口被占则失败，不静默换端口（否则 devUrl 对不上）
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"], // Tauri 约定
  build: {
    target: "chrome105", // WebView2 基线
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
