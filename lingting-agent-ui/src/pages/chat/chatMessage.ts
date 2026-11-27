import type { Message, MessageRole, ToolCallStatus } from "@lingting/agent-api";

/** 界面消息状态：已完成、流式输出中、被中断（内容可能不完整）。 */
export type ChatMessageStatus = "done" | "streaming" | "interrupted";

/** 界面消息：服务端消息的展示快照，流式内容先在本地累积。 */
export type ChatMessage = {
  kind: "message";
  key: string;
  role: MessageRole;
  content: string;
  status: ChatMessageStatus;
};

/**
 * 工具卡片的展示状态（P3-029）。
 *
 * `confirm` / `submitted` 是确认态的两步：`submitted` 之后按钮不可再点（D-P3-026）；
 * `unfinished` 是「开始事件没等到结果事件，Run 已结束」的兜底，不允许静默留成「进行中」。
 */
export type ToolStatus =
  | "running"
  | "ok"
  | "failed"
  | "cancelled"
  | "confirm"
  | "submitted"
  | "unfinished";

/** 一次工具调用的卡片：按 `toolCallId` 与三个工具事件配对。 */
export type ToolCard = {
  kind: "tool";
  key: string;
  name: string;
  status: ToolStatus;
  /** 参数摘要：服务端下发的已脱敏文本，只在确认态展示。 */
  arguments: string;
  /** 耗时（毫秒）；成功与失败后才有值。 */
  durationMs: string | null;
  /** 失败、取消或未完成的说明文本。 */
  note: string | null;
};

/** 对话流里的一项：消息气泡或工具卡片，按到达顺序排列。 */
export type ChatItem = ChatMessage | ToolCard;

/** 本地消息序号：服务端消息用真实 ID 作 key，本地占位消息用前缀避让。 */
let localSeq = 0;

/** 生成一个本地消息 key（助手占位消息用）。 */
export function nextLocalKey(prefix: string): string {
  localSeq += 1;
  return `${prefix}-${localSeq}`;
}

/** 由服务端消息生成界面消息。 */
export function fromServerMessage(message: Message): ChatMessage {
  return {
    kind: "message",
    key: message.id,
    role: message.role,
    content: message.content,
    status: "done",
  };
}

/**
 * 中断态的提示文案（P6A-015）。
 *
 * 只说「怎么继续」，不给恢复动作：客户端不需要恢复按钮，发一条消息就恢复（D-P6-009）。
 */
export function pausedNoticeOf(reason: string): string {
  return `已中断（发送消息可继续）：${reason}`;
}

/** 是否为消息项：渲染与状态迁移据此分流。 */
export function isChatMessage(item: ChatItem): item is ChatMessage {
  return item.kind === "message";
}

/** 新建一张工具卡片（未给字段取默认值）。 */
export function newToolCard(toolCallId: string, name: string): ToolCard {
  return {
    kind: "tool",
    key: toolCallId,
    name,
    status: "running",
    arguments: "",
    durationMs: null,
    note: null,
  };
}

/** 事件里的工具状态 → 卡片状态；未知取值按失败展示，不把异常当成功。 */
export function toolStatusOf(status: ToolCallStatus | null | undefined): ToolStatus {
  switch (status) {
    case "OK":
      return "ok";
    case "CANCELLED":
      return "cancelled";
    default:
      return "failed";
  }
}

/** 失败或取消的说明文本：优先用服务端下发的模型可读文本。 */
export function toolNoteOf(
  status: ToolCallStatus | null | undefined,
  error: string | null | undefined,
): string {
  if (status === "CANCELLED") return "已取消";
  if (status === "DENIED") return error || "工具未授权";
  return error || "工具执行失败";
}
