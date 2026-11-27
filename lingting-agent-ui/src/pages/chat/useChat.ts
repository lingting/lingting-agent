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
  /** 可恢复的中断态：只提示「发送消息可继续」，不提供恢复动作（P6A-015）。 */
  const [paused, setPaused] = useState<{ runId: string; reason: string } | null>(null);

  /** 当前会话 ID：首次发送时创建，之后复用。 */
  const sessionIdRef = useRef<string | null>(null);

  /** 最近一次发送产生的 Run ID；为空表示空闲。取消生成用它定位目标。 */
  const runIdRef = useRef<string | null>(null);

  /**
   * 进行中的 Run → 助手占位消息 key。
   *
   * 一次发送可能同时带出两条 Run（被恢复的旧 Run + 本次的新 Run），因此占位登记必须按 `runId`
   * 分开，不能只留一个「当前占位」（P6A-015 第 3 条）。
   */
  const runKeysRef = useRef<Map<string, string>>(new Map());

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

  /**
   * 某个 Run 的助手占位 key：没有就补一条占位（先到的增量也能落到正确位置）。
   *
   * 被恢复的旧 Run 先执行，因此它的占位插在当前（最新）Run 的占位之前：两段回复的顺序与
   * 服务端的执行顺序一致，不会因为「谁先建占位」而上下颠倒。
   */
  const ensureRunKey = useCallback((runId: string): string => {
    const existing = runKeysRef.current.get(runId);
    if (existing) return existing;
    const key = nextLocalKey("assistant");
    const currentKey = runIdRef.current ? runKeysRef.current.get(runIdRef.current) : undefined;
    runKeysRef.current.set(runId, key);
    setItems((previous) => {
      const placeholder: ChatMessage = { kind: "message", key, role: "ASSISTANT", content: "", status: "streaming" };
      const index = currentKey ? previous.findIndex((item) => item.key === currentKey) : -1;
      if (index < 0) return [...previous, placeholder];
      return [...previous.slice(0, index), placeholder, ...previous.slice(index)];
    });
    return key;
  }, []);

  /**
   * 一条 Run 结束：清掉它的登记。
   *
   * 返回是否「全部结束」——只有全部都结束才算本轮收尾，否则界面会在新 Run 刚开始时就退回空闲。
   */
  const finishRun = useCallback((runId: string): boolean => {
    runKeysRef.current.delete(runId);
    if (runKeysRef.current.size > 0) return false;
    runIdRef.current = null;
    streamingRef.current = false;
    pendingRef.current = [];
    setStreaming(false);
    return true;
  }, []);

  /** 本轮整体收尾：清掉全部登记（发送失败、连接中断这类再也收不到事件的情形）。 */
  const clearRun = useCallback(() => {
    runKeysRef.current.clear();
    runIdRef.current = null;
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

  /**
   * 把一条 Run 事件应用到对话流。
   *
   * 归类只按事件里的 `runId`：一次发送同时可能有「被恢复的旧 Run」与「本次的新 Run」两条，
   * 按到达顺序猜会把两段回复拼成一条（P6A-015）。
   */
  const applyRunEvent = useCallback(
    (event: RunEvent) => {
      if (applyToolEvent(event)) return;
      const key = ensureRunKey(event.runId);
      switch (event.event) {
        case "message_delta": {
          const delta = event.delta ?? "";
          if (delta) pushDelta(key, delta);
          break;
        }
        case "run_completed":
          finishMessage(key, "done");
          if (finishRun(event.runId)) markUnfinished();
          break;
        case "run_failed": {
          const reason = event.error ?? "Run 执行失败";
          finishMessage(key, "interrupted", failedNote(reason));
          if (finishRun(event.runId)) markUnfinished();
          setError(reason);
          break;
        }
        case "run_cancelled":
          finishMessage(key, "interrupted", stoppedRef.current ? STOPPED_NOTE : INTERRUPTED_NOTE);
          if (finishRun(event.runId)) markUnfinished();
          if (!stoppedRef.current) setError(event.error ?? "Run 已取消");
          break;
        case "server_shutdown":
          finishMessage(key, "interrupted", INTERRUPTED_NOTE);
          if (finishRun(event.runId)) markUnfinished();
          setError(event.error ?? "服务正在关闭，回复已中断");
          break;
        case "run_started":
          // 被恢复的 Run 开始执行：中断提示从此不再成立。
          setPaused((current) => (current?.runId === event.runId ? null : current));
          break;
        case "heartbeat":
          break;
      }
    },
    [applyToolEvent, ensureRunKey, finishMessage, finishRun, markUnfinished, pushDelta],
  );

  /**
   * 问一次 Run 的状态：`PAUSED` 说明上一次执行被中断、发一条消息就能继续。
   *
   * 问不到就没有提示：中断提示只是辅助信息，不因此报错。
   */
  const refreshPaused = useCallback(async (runId: string) => {
    try {
      const run = await agentApi.runGet({ runId });
      if (run.status !== "PAUSED") return;
      setPaused({ runId, reason: run.interruptedReason ?? "执行被中断" });
    } catch {
      // 状态问不到，界面保持原样。
    }
  }, []);

  /** 下行事件入口：按 `runId` 归类；连接关闭时全部收尾并提示。 */
  const onLinkEvent = useCallback(
    (linkEvent: LinkEvent) => {
      if (linkEvent.type === "closed") {
        for (const key of runKeysRef.current.values()) {
          finishMessage(key, "interrupted", INTERRUPTED_NOTE);
        }
        markUnfinished();
        const unfinished = runIdRef.current;
        clearRun();
        setError(`连接中断：${linkEvent.reason}`);
        // 连接断了但 Run 还在库里：查一次状态，中断态就给出「发一条消息即可继续」。
        if (unfinished) void refreshPaused(unfinished);
        return;
      }

      const event = linkEvent.event;
      // 空闲期没有本轮：迟到的帧（心跳、上一次的收尾）不改界面。
      if (!streamingRef.current) return;
      // 消息创建还没返回：本轮事件先缓冲，拿到 Run ID 后按序放出。
      if (runIdRef.current === null) {
        pendingRef.current.push(event);
        return;
      }
      applyRunEvent(event);
    },
    [applyRunEvent, clearRun, finishMessage, markUnfinished, refreshPaused],
  );

  useEffect(() => {
    listenerRef.current = onLinkEvent;
  }, [onLinkEvent]);

  /** 页面刷新后接着看同一条会话：历史消息与「上次是否被中断」都从服务端读回（P6A-015）。 */
  const restore = useCallback(
    async (sessionId: string) => {
      try {
        const history = await agentApi.messageList({ sessionId });
        setItems(history.map(fromServerMessage));
        const lastRun = history
          .map((message) => message.runId)
          .filter((runId): runId is string => Boolean(runId))
          .pop();
        if (lastRun) await refreshPaused(lastRun);
      } catch {
        // 历史读不回来不影响新对话：忘掉这条会话，下一次发送会新建。
        storeSession(null);
        sessionIdRef.current = null;
      }
    },
    [refreshPaused],
  );

  useEffect(() => {
    const stored = readStoredSession();
    if (!stored) return;
    sessionIdRef.current = stored;
    void restore(stored);
  }, [restore]);

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
        storeSession(sessionId);
        // 先占用会话：未被占用的会话里 Run 停在 QUEUED，不会执行（D-P1.1-004）。
        await link.occupy(sessionId);

        const sent = await agentApi.messageCreate({ content: text, sessionId });
        runIdRef.current = sent.run.id;
        // 消息创建成功即触发恢复（D-P6-009）：中断提示从此不再成立。
        setPaused(null);
        // 先登记占位再放缓冲事件：事件按 `runId` 找 key，被恢复的旧 Run 会自己补一条占位。
        runKeysRef.current.set(sent.run.id, assistantKey);
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

        // 缓冲里已攒下本轮开头的几条事件：按到达顺序放出（可能含被恢复的旧 Run 的事件）。
        for (const event of pendingRef.current.splice(0)) {
          applyRunEvent(event);
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
    const key = runId ? runKeysRef.current.get(runId) : undefined;
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

  return { cancel, clearError, confirm, error, items, messages, paused, send, streaming };
}

/** 会话 ID 的存储键：刷新页面后接着看同一条对话（P6A-015）。 */
const SESSION_KEY = "lingting.session";

/** 读回上次打开的会话 ID；没有 storage（隐私模式）时视为没有。 */
function readStoredSession(): string | null {
  try {
    return window.sessionStorage.getItem(SESSION_KEY);
  } catch {
    return null;
  }
}

/** 记住 / 忘掉当前会话 ID：存储失败只影响「刷新后能否接着看」，不影响本次对话。 */
function storeSession(sessionId: string | null): void {
  try {
    if (sessionId) window.sessionStorage.setItem(SESSION_KEY, sessionId);
    else window.sessionStorage.removeItem(SESSION_KEY);
  } catch {
    // 没有 storage 时不保存。
  }
}

/** 首次发送时创建会话：标题取第一条消息，与服务端 CLI 的 `run` 行为一致。 */
async function createSession(title: string): Promise<string> {
  const session = await agentApi.sessionCreate({ title });
  return session.id;
}
