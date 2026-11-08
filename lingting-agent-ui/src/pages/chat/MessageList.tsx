import { Bubble, type BubbleItemType } from "@ant-design/x";

import type { ChatMessage, ChatMessageStatus } from "./chatMessage";

/** 消息状态到 Bubble 状态的映射：`updating` 显示流式光标，`abort` 显示中断标记。 */
const BUBBLE_STATUS: Record<ChatMessageStatus, BubbleItemType["status"]> = {
  done: "success",
  interrupted: "abort",
  streaming: "updating",
};

/** 消息区：用户消息靠右实心，助手消息靠左描边，流式中显示光标。 */
export function MessageList({ messages }: { messages: ChatMessage[] }) {
  const items: BubbleItemType[] = messages.map((message) => ({
    key: message.key,
    role: message.role === "USER" ? "user" : "ai",
    content: message.content,
    status: BUBBLE_STATUS[message.status],
    streaming: message.status === "streaming",
    // 第一个增量到达前先显示加载态，避免出现空白气泡。
    loading: message.status === "streaming" && message.content.length === 0,
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
