import type { RunEvent } from "@lingting/agent-api";
import { useCallback, useEffect, useRef, useState } from "react";

import { type LinkEvent, agentApi, ChatLink, errorMessageOf } from "@/api";

import {
  type ChatMessage,
  type ChatMessageStatus,
  fromServerMessage,
  nextLocalKey,
} from "./chatMessage";

/** 连接中断时追加在回复末尾的说明，与服务端 TUI 的处理保持一致。 */
const INTERRUPTED_NOTE = "（连接中断，内容可能不完整）";

/** 用户主动停止生成时的说明。 */
const STOPPED_NOTE = "（已停止生成）";

/** 执行失败时追加在回复末尾的说明：与提示条共用同一份脱敏文案。 */
function failedNote(reason: string): string {
  return `（执行失败：${reason}）`;
}

/**
 * 对话状态机：注册连接、占用会话、发送消息、消费下行流、停止生成。
 *
 * 事件只有一个来源：`api/client/stream` 的下行流（D-P1.1-004）。Run 由「会话被占用 + 消息创建」
 * 触发执行，因此发送前必须先占用会话。
 */
export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
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

  /** 当前连接。 */
  const linkRef = useRef<ChatLink | null>(null);

  /** 注册中的那次连接：发送前等它完成。 */
  const connectRef = useRef<Promise<string> | null>(null);

  /** 最新的事件回调：连接对象在挂载时创建一次，回调随渲染更新。 */
  const listenerRef = useRef<(event: LinkEvent) => void>(() => {});

  const clearError = useCallback(() => setError(null), []);

  /** 收尾一条消息：可追加说明并更新状态。 */
  const finishMessage = useCallback((key: string, status: ChatMessageStatus, note = "") => {
    setMessages((previous) =>
      previous.map((message) =>
        message.key === key ? { ...message, status, content: message.content + note } : message,
      ),
    );
  }, []);

  /** 追加增量文本。 */
  const pushDelta = useCallback((key: string, delta: string) => {
    setMessages((previous) =>
      previous.map((message) =>
        message.key === key ? { ...message, content: message.content + delta } : message,
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

  /** 把一条 Run 事件应用到助手占位消息。 */
  const applyRunEvent = useCallback(
    (event: RunEvent) => {
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
          clearRun();
          break;
        case "run_failed": {
          const reason = event.error ?? "Run 执行失败";
          finishMessage(key, "interrupted", failedNote(reason));
          setError(reason);
          clearRun();
          break;
        }
        case "run_cancelled":
          finishMessage(key, "interrupted", stoppedRef.current ? STOPPED_NOTE : INTERRUPTED_NOTE);
          if (!stoppedRef.current) setError(event.error ?? "Run 已取消");
          clearRun();
          break;
        case "server_shutdown":
          finishMessage(key, "interrupted", INTERRUPTED_NOTE);
          setError(event.error ?? "服务正在关闭，回复已中断");
          clearRun();
          break;
        case "run_started":
        case "heartbeat":
          break;
      }
    },
    [clearRun, finishMessage, pushDelta],
  );

  /** 下行事件入口：只处理本轮 Run，连接关闭时收尾并提示。 */
  const onLinkEvent = useCallback(
    (linkEvent: LinkEvent) => {
      if (linkEvent.type === "closed") {
        const key = assistantKeyRef.current;
        if (key) finishMessage(key, "interrupted", INTERRUPTED_NOTE);
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
    [applyRunEvent, clearRun, finishMessage],
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
        setMessages((previous) => [
          ...previous,
          fromServerMessage(sent.message),
          { key: assistantKey, role: "ASSISTANT", content: "", status: "streaming" },
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
      clearRun();
    }
  }, [clearRun, finishMessage]);

  return { cancel, clearError, error, messages, send, streaming };
}

/** 首次发送时创建会话：标题取第一条消息，与服务端 CLI 的 `run` 行为一致。 */
async function createSession(title: string): Promise<string> {
  const session = await agentApi.sessionCreate({ title });
  return session.id;
}
