import type { ClientDecision, RunEvent } from "@lingting/agent-api";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { type LinkEvent, agentApi, ChatLink, errorMessageOf } from "@/api";

import {
  type ChatItem,
  type ChatMessage,
  type ChatMessageStatus,
  fromServerMessage,
  isChatMessage,
  newToolCard,
  nextLocalKey,
  type ToolCard,
  toolNoteOf,
  toolStatusOf,
} from "./chatMessage";

/** 连接中断时追加在回复末尾的说明，与服务端 TUI 的处理保持一致。 */
const INTERRUPTED_NOTE = "（连接中断，内容可能不完整）";

/** 用户主动停止生成时的说明。 */
const STOPPED_NOTE = "（已停止生成）";

/** 工具卡片没等到结果事件时的说明：宁可显式标未完成，也不留成「调用中」。 */
const UNFINISHED_NOTE = "Run 已结束，未收到结果";

/** 执行失败时追加在回复末尾的说明：与提示条共用同一份脱敏文案。 */
function failedNote(reason: string): string {
  return `（执行失败：${reason}）`;
}

/**
 * 对话状态机：注册连接、占用会话、发送消息、消费下行流、确认工具调用、停止生成。
 *
 * 事件只有一个来源：`api/client/stream` 的下行流（D-P1.1-004）。Run 由「会话被占用 + 消息创建」
 * 触发执行，因此发送前必须先占用会话。
 *
 * 对话流由两类项组成（[`ChatItem`]）：消息气泡与工具卡片，都按到达顺序落在同一个数组里，
 * 因此工具卡片的位置就是它在对话中真正发生的位置（P3-029）。
 */
export function useChat() {
  const [items, setItems] = useState<ChatItem[]>([]);
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /** 当前会话 ID：首次发送时创建，之后复用。 */
  const sessionIdRef = useRef<string | null>(null);

  /** 进行中的 Run ID；为空表示空闲。 */
  const runIdRef = useRef<string | null>(null);

  /** 进行中回复的助手占位消息 key。 */
  const assistantKeyRef = useRef<string | null>(null);

  /** 消息创建还没返回时先到达的下行事件：按序缓冲，不做半条拼接（D-011 第 4 条）。 */
  const pendingRef = useRef<RunEvent[]>([]);

  /** 本次生成是否被用户主动停止：用来把「停止」与「断线」区分开。 */
  const stoppedRef = useRef(false);

  /** 是否有进行中的回复（供事件回调读，避免闭包拿到旧值）。 */
  const streamingRef = useRef(false);

  /** 正在上行的确认：同一张卡片在请求落地前不重复上行（D-P3-026）。 */
  const confirmingRef = useRef<Set<string>>(new Set());

  /** 当前连接。 */
  const linkRef = useRef<ChatLink | null>(null);

  /** 注册中的那次连接：发送前等它完成。 */
  const connectRef = useRef<Promise<string> | null>(null);

  /** 最新的事件回调：连接对象在挂载时创建一次，回调随渲染更新。 */
  const listenerRef = useRef<(event: LinkEvent) => void>(() => {});

  const clearError = useCallback(() => setError(null), []);

  /** 只看消息气泡的视图：已有的消息相关调用（用例、提示）继续读它。 */
  const messages = useMemo(() => items.filter(isChatMessage), [items]);

  /** 改动某条消息项（按 key）。 */
  const updateMessage = useCallback(
    (key: string, change: (message: ChatMessage) => ChatMessage) => {
      setItems((previous) =>
        previous.map((item) => (item.kind === "message" && item.key === key ? change(item) : item)),
      );
    },
    [],
  );

  /** 改动某张工具卡片（按 `toolCallId`）；卡片不存在时什么都不做。 */
  const updateTool = useCallback((toolCallId: string, change: (card: ToolCard) => ToolCard) => {
    setItems((previous) =>
      previous.map((item) =>
        item.kind === "tool" && item.key === toolCallId ? change(item) : item,
      ),
    );
  }, []);

  /** 工具卡片：存在就改写，不存在就新建（开始事件先到，确认与结果事件后到）。 */
  const upsertTool = useCallback(
    (toolCallId: string, name: string, change: (card: ToolCard) => ToolCard) => {
      setItems((previous) => {
        const exists = previous.some((item) => item.kind === "tool" && item.key === toolCallId);
        if (!exists) return [...previous, change(newToolCard(toolCallId, name))];
        return previous.map((item) =>
          item.kind === "tool" && item.key === toolCallId ? change(item) : item,
        );
      });
    },
    [],
  );

  /** 收尾一条消息：可追加说明并更新状态。 */
  const finishMessage = useCallback(
    (key: string, status: ChatMessageStatus, note = "") => {
      updateMessage(key, (message) => ({
        ...message,
        status,
        content: message.content + note,
      }));
    },
    [updateMessage],
  );

  /** 追加增量文本。 */
  const pushDelta = useCallback(
    (key: string, delta: string) => {
      updateMessage(key, (message) => ({ ...message, content: message.content + delta }));
    },
    [updateMessage],
  );

  /** 本轮结束：仍停在进行中或待确认的卡片标未完成（不允许静默留成「调用中」）。 */
  const markUnfinished = useCallback(() => {
    setItems((previous) =>
      previous.map((item) =>
        item.kind === "tool" &&
        (item.status === "running" || item.status === "confirm" || item.status === "submitted")
          ? { ...item, status: "unfinished", note: item.note ?? UNFINISHED_NOTE }
          : item,
      ),
    );
  }, []);

  /** 本轮结束：清掉进行中的标记。 */
  const clearRun = useCallback(() => {
    runIdRef.current = null;
    assistantKeyRef.current = null;
    streamingRef.current = false;
    pendingRef.current = [];
    setStreaming(false);
  }, []);

  /** 三个工具事件按 `toolCallId` 配对；处理了就返回 `true`，不再走助手占位分支。 */
  const applyToolEvent = useCallback(
    (event: RunEvent): boolean => {
      const toolCallId = event.toolCallId;
      if (!toolCallId) return false;
      const name = event.toolName ?? "";
      switch (event.event) {
        case "tool_call_started":
          upsertTool(toolCallId, name, (card) => ({
            ...card,
            name: card.name || name,
            status: "running",
          }));
          return true;
        case "tool_call_confirm_required":
          upsertTool(toolCallId, name, (card) => ({
            ...card,
            name: card.name || name,
            status: card.status === "submitted" ? "submitted" : "confirm",
            arguments: event.toolArguments ?? "",
          }));
          return true;
        case "tool_call_completed": {
          const status = toolStatusOf(event.toolStatus);
          upsertTool(toolCallId, name, (card) => ({
            ...card,
            name: card.name || name,
            status,
            durationMs: event.toolDurationMs ?? null,
            note: status === "ok" ? null : toolNoteOf(event.toolStatus, event.error),
          }));
          // 结果已到：这张卡片不再需要确认，同标识的重复上行请求也不再放行。
          confirmingRef.current.delete(toolCallId);
          return true;
        }
        default:
          return false;
      }
    },
    [upsertTool],
  );

  /** 把一条 Run 事件应用到对话流。 */
  const applyRunEvent = useCallback(
    (event: RunEvent) => {
      if (applyToolEvent(event)) return;
      const key = assistantKeyRef.current;
      if (!key) return;
      switch (event.event) {
        case "message_delta": {
          const delta = event.delta ?? "";
          if (delta) pushDelta(key, delta);
          break;
        }
        case "run_completed":
          finishMessage(key, "done");
          markUnfinished();
          clearRun();
          break;
        case "run_failed": {
          const reason = event.error ?? "Run 执行失败";
          finishMessage(key, "interrupted", failedNote(reason));
          markUnfinished();
          setError(reason);
          clearRun();
          break;
        }
        case "run_cancelled":
          finishMessage(key, "interrupted", stoppedRef.current ? STOPPED_NOTE : INTERRUPTED_NOTE);
          markUnfinished();
          if (!stoppedRef.current) setError(event.error ?? "Run 已取消");
          clearRun();
          break;
        case "server_shutdown":
          finishMessage(key, "interrupted", INTERRUPTED_NOTE);
          markUnfinished();
          setError(event.error ?? "服务正在关闭，回复已中断");
          clearRun();
          break;
        case "run_started":
        case "heartbeat":
          break;
      }
    },
    [applyToolEvent, clearRun, finishMessage, markUnfinished, pushDelta],
  );

  /** 下行事件入口：只处理本轮 Run，连接关闭时收尾并提示。 */
  const onLinkEvent = useCallback(
    (linkEvent: LinkEvent) => {
      if (linkEvent.type === "closed") {
        const key = assistantKeyRef.current;
        if (key) finishMessage(key, "interrupted", INTERRUPTED_NOTE);
        markUnfinished();
        clearRun();
        setError(`连接中断：${linkEvent.reason}`);
        return;
      }

      const event = linkEvent.event;
      // 消息创建还没返回：本轮事件先缓冲，拿到 Run ID 后按序放出。
      if (runIdRef.current === null) {
        if (streamingRef.current) pendingRef.current.push(event);
        return;
      }
      if (event.runId !== runIdRef.current) return;
      applyRunEvent(event);
    },
    [applyRunEvent, clearRun, finishMessage, markUnfinished],
  );

  useEffect(() => {
    listenerRef.current = onLinkEvent;
  }, [onLinkEvent]);

  // 连接生命周期与页面一致：挂载即注册并建立下行流，卸载时交还占用、反注册、停心跳。
  useEffect(() => {
    const link = new ChatLink(agentApi, (event) => listenerRef.current(event));
    linkRef.current = link;
    const connecting = link.connect();
    connectRef.current = connecting;
    // 发送时还会再等一次这个 promise，这里只兜住未处理的失败。
    connecting.catch((failure: unknown) => setError(errorMessageOf(failure)));
    return () => {
      linkRef.current = null;
      connectRef.current = null;
      void link.release();
    };
  }, []);

  const send = useCallback(
    async (content: string) => {
      const text = content.trim();
      if (!text || streamingRef.current) return;
      const link = linkRef.current;
      const connecting = connectRef.current;
      if (!link || !connecting) {
        setError("连接尚未建立，请刷新页面重试");
        return;
      }

      stoppedRef.current = false;
      streamingRef.current = true;
      pendingRef.current = [];
      setError(null);
      setStreaming(true);

      const assistantKey = nextLocalKey("assistant");
      try {
        // 等待注册完成：没有 clientId 就无法建立占用。
        await connecting;
        const sessionId = sessionIdRef.current ?? (await createSession(text));
        sessionIdRef.current = sessionId;
        // 先占用会话：未被占用的会话里 Run 停在 QUEUED，不会执行（D-P1.1-004）。
        await link.occupy(sessionId);

        const sent = await agentApi.messageCreate({ content: text, sessionId });
        runIdRef.current = sent.run.id;
        assistantKeyRef.current = assistantKey;
        setItems((previous) => [
          ...previous,
          fromServerMessage(sent.message),
          {
            kind: "message",
            key: assistantKey,
            role: "ASSISTANT",
            content: "",
            status: "streaming",
          },
        ]);

        // 缓冲里已攒下本轮开头的几条事件：按到达顺序放出。
        for (const event of pendingRef.current.splice(0)) {
          if (event.runId === sent.run.id) applyRunEvent(event);
        }
      } catch (failure) {
        setError(errorMessageOf(failure));
        clearRun();
      }
    },
    [applyRunEvent, clearRun],
  );

  /**
   * 确认一次工具调用（D-P3-026）：点即进入「等结果」态，请求落地前不重复上行。
   *
   * 上行失败时把卡片退回可确认态：用户换一个决定或重试即可，不必刷新页面。
   */
  const confirm = useCallback(
    async (toolCallId: string, decision: ClientDecision) => {
      const link = linkRef.current;
      if (!link) {
        setError("连接尚未建立，请刷新页面重试");
        return;
      }
      if (confirmingRef.current.has(toolCallId)) return;
      confirmingRef.current.add(toolCallId);
      updateTool(toolCallId, (card) =>
        card.status === "confirm" ? { ...card, status: "submitted" } : card,
      );
      try {
        await link.confirm(toolCallId, decision);
      } catch (failure) {
        setError(errorMessageOf(failure));
        updateTool(toolCallId, (card) =>
          card.status === "submitted" ? { ...card, status: "confirm" } : card,
        );
      } finally {
        confirmingRef.current.delete(toolCallId);
      }
    },
    [updateTool],
  );

  /**
   * 停止生成：先让服务端把 Run 落成取消，再本地收尾。
   *
   * 取消后服务端不再为该 Run 产出事件，这条下行流不会有终态事件，因此不等事件到来。
   */
  const cancel = useCallback(async () => {
    const runId = runIdRef.current;
    stoppedRef.current = true;
    const key = assistantKeyRef.current;
    try {
      if (runId) await agentApi.runCancel({ runId });
    } catch (failure) {
      setError(errorMessageOf(failure));
    } finally {
      if (key) finishMessage(key, "interrupted", STOPPED_NOTE);
      markUnfinished();
      clearRun();
    }
  }, [clearRun, finishMessage, markUnfinished]);

  return { cancel, clearError, confirm, error, items, messages, send, streaming };
}

/** 首次发送时创建会话：标题取第一条消息，与服务端 CLI 的 `run` 行为一致。 */
async function createSession(title: string): Promise<string> {
  const session = await agentApi.sessionCreate({ title });
  return session.id;
}
