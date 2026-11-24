import { Bubble, type BubbleItemType } from "@ant-design/x";

import { type ChatItem, type ChatMessage, type ChatMessageStatus, type ToolCard } from "./chatMessage";
import { type ToolConfirmHandler, ToolCardView } from "./ToolCard";

/** 消息状态到 Bubble 状态的映射：`updating` 显示流式光标，`abort` 显示中断标记。 */
const BUBBLE_STATUS: Record<ChatMessageStatus, BubbleItemType["status"]> = {
  done: "success",
  interrupted: "abort",
  streaming: "updating",
};

/** 第一个增量到达之前的提示文案：Run 已创建但还没有开始输出。 */
const WAITING_TEXT = "等待执行…";

/** 消息项：用户消息靠右实心，助手消息靠左描边，流式中显示光标。 */
function messageItem(message: ChatMessage): BubbleItemType {
  return {
    key: message.key,
    role: message.role === "USER" ? "user" : "ai",
    // 第一个增量到达前先给「等待执行…」，避免空白气泡；正文一到位就换成真实内容。
    content: message.content || (message.status === "streaming" ? WAITING_TEXT : ""),
    status: BUBBLE_STATUS[message.status],
    streaming: message.status === "streaming",
  };
}

/** 工具项：一张无边框的紧凑卡片，与消息气泡同一列，按到达顺序插入（P3-029）。 */
function toolItem(card: ToolCard, onConfirm: ToolConfirmHandler): BubbleItemType {
  return {
    key: card.key,
    role: "ai",
    variant: "borderless",
    content: <ToolCardView card={card} onConfirm={onConfirm} />,
  };
}

/** 对话流：消息气泡与工具卡片按到达顺序排在同一列里。 */
export function MessageList({ items, onConfirm }: { items: ChatItem[]; onConfirm: ToolConfirmHandler }) {
  const list: BubbleItemType[] = items.map((item) =>
    item.kind === "tool" ? toolItem(item, onConfirm) : messageItem(item),
  );

  return (
    <Bubble.List
      autoScroll
      className="h-full"
      items={list}
      role={{
        ai: { placement: "start", shape: "corner", variant: "outlined" },
        user: { placement: "end", shape: "corner", variant: "filled" },
      }}
    />
  );
}
