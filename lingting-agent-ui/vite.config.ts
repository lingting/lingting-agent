import { resolve } from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite-plus";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    // LRI 以 npm 包引入，pnpm 会为它的 peerDependencies 单独安装副本，导致同一个包在本工程与
    // LRI 下各存在一份实例。dev 下 Vite 预打包会把裸模块说明符统一改写为同一 chunk，问题被掩盖；
    // 构建（rolldown）按 importer 各自解析，会产出两份实现，React Context 与模块级单例无法跨实例共享。
    //
    // 维护规则：本列表必须与 lingting-react-ui/package.json 的 peerDependencies 包名保持一致，
    // LRI 新增 peer 依赖时必须同步补齐。匹配基于包名，深层导入（如 react/jsx-runtime）已由包名覆盖。
    dedupe: [
      "@ant-design/cssinjs",
      "@ant-design/icons",
      "@ant-design/plots",
      "@ant-design/pro-components",
      "@ant-design/x",
      "@ant-design/x-markdown",
      "@ant-design/x-sdk",
      "@marsidev/react-turnstile",
      "@tanstack/react-query",
      "@tanstack/react-router",
      "@tanstack/react-store",
      "antd",
      "clsx",
      "dayjs",
      "react",
      "react-dom",
    ],
    alias: {
      "@": resolve(import.meta.dirname, "src"),
      "@lri": "lingting-react-ui",
    },
  },
  fmt: {
    ignorePatterns: [".agents", "node_modules", "dist", "*.yaml", "*.md"],
  },
  server: {
    host: "127.0.0.1",
    // 固定端口，见 docs/develop.md 与 .steps/p1/decisions.md D-022。
    port: 20101,
    proxy: {
      // 必须带尾部斜杠：vite 的代理 key 按前缀匹配，写成 "/api" 会把前端路由 /api-key 也转发给后端。
      // 后端路由本身已带 api/ 前缀（如 api/probe/void），因此不做 rewrite。
      "^/api/": {
        target: "http://127.0.0.1:20102",
        changeOrigin: true,
      },
    },
  },
});
