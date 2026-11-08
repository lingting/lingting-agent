import { defineConfig } from "@playwright/test";

/**
 * 端到端用例配置。
 *
 * 与单元用例的分工：单元用例（`tests/*.test.ts`）跑在 jsdom 里，端到端用例（`tests/e2e/`）跑真实
 * Chromium，用来验收「发消息看到流式」「停止生成」「关服后界面有可读错误」。
 *
 * **不启动任何 webServer**：服务端是 Rust 进程，由 `bin-tests` 的 `web.e2e` 用例拉起
 * （静态资源也由它托管），地址通过 `WEBUI_BASE_URL` 传进来。手动跑时把服务端起在本机即可：
 *
 * ```text
 * make ui-build && cargo run -p bin-agent -- server    # debug 默认 20102
 * cd lingting-agent-ui && pnpm exec playwright test
 * ```
 *
 * 想连开发服务器（改前端后不用重新构建）就把 `WEBUI_BASE_URL` 指向 `pnpm dev` 的地址：
 *
 * ```text
 * make ui-dev                                          # 20101，代理到 20102
 * cd lingting-agent-ui && WEBUI_BASE_URL=http://127.0.0.1:20101 pnpm exec playwright test
 * ```
 */
const baseURL = process.env.WEBUI_BASE_URL ?? "http://127.0.0.1:20102";

export default defineConfig({
  testDir: "./tests/e2e",
  // 失败截图与追踪落到固定目录，由 .gitignore 忽略。
  outputDir: "./tests/.artifacts",
  reporter: [["list"]],
  // 对话用例互不依赖，可以并行；单个 worker 只跑一个浏览器上下文。
  fullyParallel: true,
  workers: 2,
  retries: 0,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  use: {
    baseURL,
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chat",
      testMatch: /chat\.spec\.ts/,
      use: { browserName: "chromium" },
    },
    {
      // 关服用例会把服务端停掉，必须等对话用例全部结束之后再跑。
      name: "shutdown",
      testMatch: /shutdown\.spec\.ts/,
      dependencies: ["chat"],
      use: { browserName: "chromium" },
    },
  ],
});
