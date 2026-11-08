/**
 * 对话页端到端用例：真实浏览器 + 真实服务端（由 bin-tests 的 web.e2e 拉起）。
 *
 * 关服用例会把服务端停掉，所以整个文件串行，且它必须是最后一条。
 */

import { expect, test } from "@playwright/test";

/** 输入框的占位文案（与 Composer 保持一致）。 */
const PLACEHOLDER = "输入消息，Enter 发送，Shift + Enter 换行";

/** 生成中显示停止按钮；发送按钮是同一位置的箭头。 */
const STOP_BUTTON = ".ant-sender-actions-btn-loading-button";

/** 助手气泡。 */
const ASSISTANT_BUBBLE = ".ant-bubble-start";

test.describe.configure({ mode: "serial" });

test("发送消息后逐段看到流式回复", async ({ page }) => {
  await page.goto("/");

  const input = page.getByPlaceholder(PLACEHOLDER);
  await input.fill("端到端测试");
  await input.press("Enter");

  // 用户消息立刻上屏。
  await expect(page.getByText("端到端测试", { exact: true })).toBeVisible();

  const bubble = page.locator(ASSISTANT_BUBBLE).last();
  // 回复开头先到：此时正文还没写完。
  await expect(bubble).toContainText("收到你的消息：端到端测试");
  const partial = await bubble.innerText();

  // 等正文全部到齐，再比较长度，证明是「逐段变长」而不是一次性到位。
  await expect(bubble).toContainText("P2 接入 Agent Runtime");
  expect((await bubble.innerText()).length).toBeGreaterThan(partial.length);

  // 结束后回到可发送状态。
  await expect(page.locator(STOP_BUTTON)).toHaveCount(0);
});

test("停止生成后保留已收内容并打标", async ({ page }) => {
  await page.goto("/");

  const input = page.getByPlaceholder(PLACEHOLDER);
  await input.fill("停止测试");
  await input.press("Enter");

  const stop = page.locator(STOP_BUTTON);
  await expect(stop).toBeVisible();
  await stop.click();

  const bubble = page.locator(ASSISTANT_BUBBLE).last();
  await expect(bubble).toContainText("（已停止生成）");
  await expect(stop).toHaveCount(0);

  // 停止后仍可继续输入。
  await expect(input).toHaveValue("");
});

test("服务端关闭后界面给出可读错误", async ({ page }) => {
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
});
