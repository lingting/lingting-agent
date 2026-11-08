import { useCallback, useRef, useState } from "react";

import { agentApi, errorMessageOf, isAborted, subscribeRunEventStream } from "@/api";

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

/** 对话状态机：会话创建、发送消息、消费事件流、停止生成。 */
export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /** 当前会话 ID：首次发送时创建，之后复用。 */
  const sessionIdRef = useRef<string | null>(null);

  /** 进行中的 Run ID；为空表示空闲。 */
  const runIdRef = useRef<string | null>(null);

  /** 进行中的事件流：停止生成时直接中断。 */
  const abortRef = useRef<AbortController | null>(null);

  /** 本次生成是否被用户主动停止：用来把「停止」与「断线」区分开。 */
  const stoppedRef = useRef(false);

  const clearError = useCallback(() => setError(null), []);

  /** 收尾一条消息：可追加说明并更新状态。 */
  const finishMessage = useCallback((key: string, status: ChatMessageStatus, note = "") => {
    setMessages((previous) =>
      previous.map((message) =>
        message.key === key ? { ...message, status, content: message.content + note } : message,
      ),
    );
  }, []);

  /** 消费事件流：增量直接追加到助手占位消息。 */
  const consume = useCallback(
    async (runId: string, key: string) => {
      const abort = new AbortController();
      abortRef.current = abort;

      let completed = false;
      let failure: string | null = null;

      for await (const event of subscribeRunEventStream(agentApi, { runId }, abort.signal)) {
        switch (event.event) {
          case "message_delta": {
            const delta = event.delta ?? "";
            if (!delta) break;
            setMessages((previous) =>
              previous.map((message) =>
                message.key === key ? { ...message, content: message.content + delta } : message,
              ),
            );
            break;
          }
          case "run_completed":
            completed = true;
            break;
          case "run_failed":
            failure = event.error ?? "Run 执行失败";
            break;
          case "server_shutdown":
            failure = event.error ?? "服务正在关闭，回复已中断";
            break;
          case "run_started":
          case "heartbeat":
            break;
        }
        // 结束类事件到达后立即收尾：服务端随后会关闭连接，不必等它断开。
        if (completed || failure !== null) break;
      }

      if (completed) {
        finishMessage(key, "done");
        return;
      }
      if (stoppedRef.current) {
        finishMessage(key, "interrupted", STOPPED_NOTE);
        return;
      }
      finishMessage(key, "interrupted", INTERRUPTED_NOTE);
      setError(failure ?? "事件流意外结束，回复可能不完整");
    },
    [finishMessage],
  );

  const send = useCallback(
    async (content: string) => {
      const text = content.trim();
      if (!text || runIdRef.current) return;
      stoppedRef.current = false;

      setError(null);
      setStreaming(true);

      const assistantKey = nextLocalKey("assistant");
      try {
        const sessionId = sessionIdRef.current ?? (await createSession(text));
        sessionIdRef.current = sessionId;

        const sent = await agentApi.messageCreate({ content: text, sessionId });
        runIdRef.current = sent.run.id;
        setMessages((previous) => [
          ...previous,
          fromServerMessage(sent.message),
          { key: assistantKey, role: "ASSISTANT", content: "", status: "streaming" },
        ]);

        await consume(sent.run.id, assistantKey);
      } catch (failure) {
        if (isAborted(failure)) {
          finishMessage(assistantKey, "interrupted", STOPPED_NOTE);
        } else {
          setError(errorMessageOf(failure));
          finishMessage(assistantKey, "interrupted", INTERRUPTED_NOTE);
        }
      } finally {
        runIdRef.current = null;
        abortRef.current = null;
        setStreaming(false);
      }
    },
    [consume, finishMessage],
  );

  /**
   * 停止生成：先让服务端把 Run 落成取消（服务端会在下一段增量前停推并结束事件流），
   * 再断开本地事件流，避免「先断流 → 服务端已自行取消 → 取消请求返回 400」的竞态。
   */
  const cancel = useCallback(async () => {
    const runId = runIdRef.current;
    stoppedRef.current = true;
    try {
      if (runId) await agentApi.runCancel({ runId });
    } catch (failure) {
      setError(errorMessageOf(failure));
    } finally {
      abortRef.current?.abort();
    }
  }, []);

  return { cancel, clearError, error, messages, send, streaming };
}

/** 首次发送时创建会话：标题取第一条消息，与服务端 CLI 的 `run` 行为一致。 */
async function createSession(title: string): Promise<string> {
  const session = await agentApi.sessionCreate({ title });
  return session.id;
}
