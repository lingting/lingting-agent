import type { Message, MessageRole } from "@lingting/agent-api";

/** 界面消息状态：已完成、流式输出中、被中断（内容可能不完整）。 */
export type ChatMessageStatus = "done" | "streaming" | "interrupted";

/** 界面消息：服务端消息的展示快照，流式内容先在本地累积。 */
export type ChatMessage = {
  key: string;
  role: MessageRole;
  content: string;
  status: ChatMessageStatus;
};

/** 本地消息序号：服务端消息用真实 ID 作 key，本地占位消息用前缀避让。 */
let localSeq = 0;

/** 生成一个本地消息 key（助手占位消息用）。 */
export function nextLocalKey(prefix: string): string {
  localSeq += 1;
  return `${prefix}-${localSeq}`;
}

/** 由服务端消息生成界面消息。 */
export function fromServerMessage(message: Message): ChatMessage {
  return { key: message.id, role: message.role, content: message.content, status: "done" };
}
