/** 消息区渲染用例：第一个增量到达之前的等待提示与正文切换。 */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vite-plus/test";

import type { ChatMessage, ChatMessageStatus } from "@/pages/chat/chatMessage";
import { MessageList } from "@/pages/chat/MessageList";

/** 等待执行提示的文案（与 MessageList 保持一致）。 */
const WAITING_TEXT = "等待执行…";

/** 一条助手消息。 */
function assistant(content: string, status: ChatMessageStatus): ChatMessage {
  return { content, key: "a-1", role: "ASSISTANT", status };
}

/** 每个用例结束后卸载：没有全局自动清理，残留 DOM 会让 `screen` 查询串台。 */
afterEach(cleanup);

describe("MessageList", () => {
  it("第一个增量到达之前显示等待执行提示", () => {
    render(<MessageList messages={[assistant("", "streaming")]} />);

    expect(screen.getByText(WAITING_TEXT)).toBeTruthy();
  });

  it("正文到达之后不再显示等待执行提示", () => {
    render(<MessageList messages={[assistant("半句", "streaming")]} />);

    expect(screen.queryByText(WAITING_TEXT)).toBeNull();
    expect(screen.getByText("半句")).toBeTruthy();
  });
});
