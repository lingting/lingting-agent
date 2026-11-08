import { defineConfig } from "@playwright/test";

/**
 * 端到端用例配置。
 *
 * 与单元用例的分工：单元用例（`tests/*.test.ts`）跑在 jsdom 里，端到端用例（`tests/e2e/`）跑真实
 * Chromium，用来验收「发消息看到流式」「停止生成」「关服后界面有可读错误」。
 *
 * **不启动任何 webServer**：服务端是 Rust 进程，由 `bin-tests` 的 `web.e2e` 用例拉起
 * （静态资源也由它托管），地址通过 `WEBUI_BASE_URL` 传进来。手动跑时先把服务端起在本机：
 *
 * ```text
 * make ui-build && cargo run -p bin-agent -- server --port 20100
 * pnpm test:e2e
 * ```
 */
const baseURL = process.env.WEBUI_BASE_URL ?? "http://127.0.0.1:20100";

export default defineConfig({
  testDir: "./tests/e2e",
  // 失败截图与追踪落到固定目录，由 .gitignore 忽略。
  outputDir: "./tests/.artifacts",
  reporter: [["list"]],
  // 所有用例共用同一个服务端；关服用例会把服务端停掉，因此必须串行。
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  use: {
    baseURL,
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { browserName: "chromium" } }],
});
