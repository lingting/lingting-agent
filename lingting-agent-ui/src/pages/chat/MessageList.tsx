import { Bubble, type BubbleItemType } from "@ant-design/x";

import type { ChatMessage, ChatMessageStatus } from "./chatMessage";

/** 消息状态到 Bubble 状态的映射：`updating` 显示流式光标，`abort` 显示中断标记。 */
const BUBBLE_STATUS: Record<ChatMessageStatus, BubbleItemType["status"]> = {
  done: "success",
  interrupted: "abort",
  streaming: "updating",
};

/** 第一个增量到达之前的提示文案：Run 已创建但还没有开始输出。 */
const WAITING_TEXT = "等待执行…";

/** 消息区：用户消息靠右实心，助手消息靠左描边，流式中显示光标。 */
export function MessageList({ messages }: { messages: ChatMessage[] }) {
  const items: BubbleItemType[] = messages.map((message) => ({
    key: message.key,
    role: message.role === "USER" ? "user" : "ai",
    // 第一个增量到达前先给「等待执行…」，避免空白气泡；正文一到位就换成真实内容。
    content: message.content || (message.status === "streaming" ? WAITING_TEXT : ""),
    status: BUBBLE_STATUS[message.status],
    streaming: message.status === "streaming",
  }));

  return (
    <Bubble.List
      autoScroll
      className="h-full"
      items={items}
      role={{
        ai: { placement: "start", shape: "corner", variant: "outlined" },
        user: { placement: "end", shape: "corner", variant: "filled" },
      }}
    />
  );
}
