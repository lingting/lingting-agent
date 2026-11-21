/**
 * 关服端到端用例：会把服务端停掉，因此单独成文件。
 *
 * `playwright.config.ts` 里把它的项目声明成依赖对话项目，保证在对话用例之后执行。
 * 控制台错误只放行浏览器自己上报的连接失败文案（W-1）。
 */

import { expect, test } from "@playwright/test";

import { SHUTDOWN_ERRORS, expectNoConsoleError, watchConsole } from "./console";

/** 输入框的占位文案（与 Composer 保持一致）。 */
const PLACEHOLDER = "输入消息，Enter 发送，Shift + Enter 换行";

test("服务端关闭后界面给出可读错误", async ({ page }) => {
  const errors = watchConsole(page);
  await page.goto("/");

  // 关闭探针只在 debug 构建注册；bin-tests 起的服务端就是 debug 构建。
  const shutdown = await page.request.post("/api/probe/shutdown");
  expect(shutdown.status()).toBe(204);

  const input = page.getByPlaceholder(PLACEHOLDER);
  await input.fill("关服测试");
  await input.press("Enter");

  // 拒绝窗口内是 503 文案，窗口过后是连不上；两种都必须可读，不能白屏。
  const alert = page.locator(".ant-alert");
  await expect(alert).toBeVisible();
  await expect(alert).toContainText(/无法连接到服务端|服务正在关闭/);
  // 页面仍然活着：输入框还在，没有被清空成白屏。
  await expect(input).toBeVisible();
  expectNoConsoleError(errors, SHUTDOWN_ERRORS);
});
