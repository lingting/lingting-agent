/**
 * 浏览器控制台断言（P2-028 / W-1）。
 *
 * 收集 `console.error` 与未捕获的页面异常，用例末尾交给 [`expectNoConsoleError`] 断言为空。
 * 服务端被主动关掉时浏览器会自己上报网络错误，这类已知文案必须在用例里显式放行，
 * 不能写成「什么都过」的宽正则。
 */

import { expect, type Page } from "@playwright/test";

/** 关服场景下允许出现的控制台错误：浏览器上报的连接失败，与界面逻辑无关。 */
export const SHUTDOWN_ERRORS = [/net::ERR_CONNECTION_REFUSED/, /Failed to load resource/];

/** 开始收集控制台错误。 */
export function watchConsole(page: Page): string[] {
  const errors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  page.on("pageerror", (error) => errors.push(`页面异常：${error.message}`));
  return errors;
}

/** 断言收集到的错误为空；`allowed` 只放行显式列出的已知文案。 */
export function expectNoConsoleError(errors: string[], allowed: RegExp[] = []): void {
  const unexpected = errors.filter((text) => !allowed.some((pattern) => pattern.test(text)));
  expect(unexpected, `浏览器控制台出现未预期的错误：${unexpected.join(" | ")}`).toEqual([]);
}
