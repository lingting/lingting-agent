/**
 * 工具卡片端到端用例（W-3 的浏览器侧，P3-029 的「视觉层级」条目）。
 *
 * 服务端由 `bin-tests` 的 `tool.web.e2e` 拉起，并把 `calculator` 配成「需确认」：因此在真实
 * Chromium 里能走完「待确认 → 允许 → 完成」，同时把两个状态的截图落在 `tests/.artifacts/`
 * （已被 `.gitignore` 忽略），供人工目测比对卡片与消息气泡的视觉层级。
 *
 * 断言口径：只断言「与既有排版一致」这一层（同一字号、卡片宽度不超过气泡列宽），不绑定
 * 具体的间距数值——间距由 antd 主题给出，写死数字会让主题调整时假失败。
 */

import { expect, test, type Locator } from "@playwright/test";

import { expectNoConsoleError, watchConsole } from "./console";

/** 输入框的占位文案（与 Composer 保持一致）。 */
const PLACEHOLDER = "输入消息，Enter 发送，Shift + Enter 换行";

/** 用户气泡：确认态下还没有助手正文，字号比对以它为准。 */
const USER_BUBBLE = ".ant-bubble-end";

/** 触发一次 `calculator` 调用的 Mock 脚本（脚本必须落在消息末尾，见 bin-tests 的 Mock）。 */
const PROMPT = `走一次工具调用\ntool:{"calls":[{"name":"calculator","arguments":{"expression":"(2+3)*4"}}]}`;

/** 截图的落地目录：Playwright 的 `outputDir` 同级，`.gitignore` 已忽略。 */
const ARTIFACTS = "tests/.artifacts";

test("工具卡片走完待确认到完成的确认链路", async ({ page }) => {
  const errors = watchConsole(page);
  await page.goto("/");

  const input = page.getByPlaceholder(PLACEHOLDER);
  await input.fill(PROMPT);
  await input.press("Enter");

  // 待确认态：工具名、状态标签、参数摘要与两个按钮都在同一张卡片里。
  const card = page.locator(".ant-card").last();
  await expect(card).toContainText("calculator");
  await expect(card).toContainText("待确认");
  // 状态标签用 antd 的语义预设色（待确认 = warning，完成 = success），不新增颜色常量；
  // 具体色值由主题决定（本项目的 `colorSuccess` 是琥珀色系，不是 antd 默认的绿），故只核类名。
  const tag = card.locator(".ant-tag");
  await expect(tag).toHaveClass(/ant-tag-warning/);
  await expect(card).toContainText("(2+3)*4");
  // antd 会在两个汉字之间插一个空格（`允 许`），所以按正则可访问名匹配。
  const allow = card.getByRole("button", { name: /允\s*许/ });
  await expect(allow).toBeEnabled();
  await expect(card.getByRole("button", { name: /拒\s*绝/ })).toBeEnabled();

  // 视觉层级：卡片正文与消息气泡同一字号，卡片宽度不超过气泡列的 560px 上限。
  expect(await fontSizeOf(card)).toBe(await fontSizeOf(page.locator(USER_BUBBLE).first()));
  expect(await maxWidthOf(card)).toBe("560px");

  await page.screenshot({ path: `${ARTIFACTS}/tool-card-confirm.png` });

  // 允许之后：Run 重新执行，卡片转「完成」并带耗时，两个按钮随之消失（`ToolCard` 只在
  // 待确认与等结果两态渲染按钮）。
  await allow.click();
  await expect(card).toContainText("完成");
  await expect(tag).toHaveClass(/ant-tag-success/);
  await expect(allow).toHaveCount(0);
  // 卡片本身也坐在 `.ant-bubble-start` 里（与助手气泡同列），因此按正文定位助手回复。
  await expect(page.getByText("Mock 回复：20")).toBeVisible();

  await page.screenshot({ path: `${ARTIFACTS}/tool-card-done.png` });

  expectNoConsoleError(errors);
});

/** 元素的计算字号。 */
function fontSizeOf(locator: Locator): Promise<string> {
  return locator.evaluate((element) => getComputedStyle(element).fontSize);
}

/** 元素的计算最大宽度。 */
function maxWidthOf(locator: Locator): Promise<string> {
  return locator.evaluate((element) => getComputedStyle(element).maxWidth);
}
