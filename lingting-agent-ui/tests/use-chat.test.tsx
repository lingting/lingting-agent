/** 对话状态机用例：成功流式、用户停止、服务关闭与断线。 */

import type { Message, MessageSendVO, Run, RunEvent, SessionSummary } from "@lingting/agent-api";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { agentApi, ApiRequestError, subscribeRunEventStream } from "@/api";
import { useChat } from "@/pages/chat/useChat";

vi.mock("@/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api")>();
  return {
    ...actual,
    agentApi: { messageCreate: vi.fn(), runCancel: vi.fn(), sessionCreate: vi.fn() },
    subscribeRunEventStream: vi.fn(),
  };
});

const api = agentApi as unknown as {
  messageCreate: ReturnType<typeof vi.fn>;
  runCancel: ReturnType<typeof vi.fn>;
  sessionCreate: ReturnType<typeof vi.fn>;
};
const openStream = subscribeRunEventStream as unknown as ReturnType<typeof vi.fn>;

/** 会话摘要。 */
function session(): SessionSummary {
  return {
    id: "s-1",
    messageCount: "0",
    createdAtMs: "1767225600000",
    model: "mock-model",
    title: "测试会话",
    updatedAtMs: "1767225600000",
  };
}

/** 用户消息。 */
function userMessage(content: string): Message {
  return {
    content,
    createdAtMs: "1767225600000",
    id: "m-1",
    inputTokens: null,
    model: null,
    outputTokens: null,
    role: "USER",
    runId: "r-1",
    sessionId: "s-1",
  };
}

/** 一次 Run。 */
function run(status: Run["status"]): Run {
  return {
    createdAtMs: "1767225600000",
    error: null,
    finishedAtMs: null,
    id: "r-1",
    inputTokens: null,
    model: "mock-model",
    outputTokens: null,
    sessionId: "s-1",
    status,
    updatedAtMs: "1767225600000",
  };
}

/** 发送消息的响应。 */
function sent(): MessageSendVO {
  return { message: userMessage("你好"), run: run("QUEUED") };
}

/** 一条事件。 */
function event(
  kind: RunEvent["event"],
  delta: string | null = null,
  error: string | null = null,
): RunEvent {
  return {
    createdAtMs: "1767225600000",
    delta,
    error,
    event: kind,
    inputTokens: null,
    messageId: null,
    outputTokens: null,
    runId: "r-1",
    seq: "0",
    sessionId: "s-1",
    status: null,
  };
}

/** 让事件流按顺序产出给定事件后结束。 */
function streamOf(events: RunEvent[]) {
  openStream.mockImplementation(() =>
    (async function* () {
      for (const item of events) yield item;
    })(),
  );
}

/** 让事件流在收到 abort 前一直挂住（模拟「正在输出」）。 */
function hangingStream(events: RunEvent[]) {
  openStream.mockImplementation((_client, _params, signal: AbortSignal) =>
    (async function* () {
      for (const item of events) yield item;
      await new Promise((resolve) => signal.addEventListener("abort", resolve));
    })(),
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  api.sessionCreate.mockResolvedValue(session());
  api.messageCreate.mockResolvedValue(sent());
  api.runCancel.mockResolvedValue(run("CANCELLED"));
});

describe("useChat", () => {
  it("首次发送创建会话、追加用户消息并累计流式增量", async () => {
    streamOf([
      event("run_started"),
      event("message_delta", "你"),
      event("message_delta", "好"),
      event("run_completed"),
    ]);
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("  你好  ");
    });

    expect(api.sessionCreate).toHaveBeenCalledWith({ title: "你好" });
    expect(api.messageCreate).toHaveBeenCalledWith({ content: "你好", sessionId: "s-1" });
    expect(result.current.messages.map((message) => message.role)).toEqual(["USER", "ASSISTANT"]);
    expect(result.current.messages[1].content).toBe("你好");
    expect(result.current.messages[1].status).toBe("done");
    expect(result.current.error).toBeNull();
    expect(result.current.streaming).toBe(false);
  });

  it("第二次发送复用已创建的会话", async () => {
    streamOf([event("run_completed")]);
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("第一条");
    });
    await act(async () => {
      await result.current.send("第二条");
    });

    expect(api.sessionCreate).toHaveBeenCalledTimes(1);
    expect(api.messageCreate).toHaveBeenCalledTimes(2);
    expect(api.messageCreate).toHaveBeenLastCalledWith({ content: "第二条", sessionId: "s-1" });
  });

  it("空白内容不发起任何请求", async () => {
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("   ");
    });

    expect(api.sessionCreate).not.toHaveBeenCalled();
    expect(api.messageCreate).not.toHaveBeenCalled();
    expect(result.current.streaming).toBe(false);
  });

  it("停止生成：先取消 Run 再断开事件流，并给回复打标", async () => {
    hangingStream([event("run_started"), event("message_delta", "半句")]);
    const { result } = renderHook(() => useChat());

    act(() => {
      void result.current.send("你好");
    });
    await waitFor(() => expect(result.current.messages[1]?.content).toBe("半句"));

    await act(async () => {
      await result.current.cancel();
    });

    expect(api.runCancel).toHaveBeenCalledWith({ runId: "r-1" });
    await waitFor(() => expect(result.current.streaming).toBe(false));
    expect(result.current.messages[1].content).toBe("半句（已停止生成）");
    expect(result.current.messages[1].status).toBe("interrupted");
    expect(result.current.error).toBeNull();
  });

  it("服务关闭事件把回复标成中断并给出原因", async () => {
    streamOf([
      event("run_started"),
      event("message_delta", "半句"),
      event("server_shutdown", null, "服务正在关闭，事件流已终止"),
    ]);
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("你好");
    });

    expect(result.current.messages[1].status).toBe("interrupted");
    expect(result.current.messages[1].content).toBe("半句（连接中断，内容可能不完整）");
    expect(result.current.error).toBe("服务正在关闭，事件流已终止");
  });

  it("事件流报错时保留已收内容并展示可读错误", async () => {
    openStream.mockImplementation(() =>
      (async function* () {
        yield event("run_started");
        throw new ApiRequestError("network", "无法连接到服务端（同源），请确认服务已启动");
      })(),
    );
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("你好");
    });

    expect(result.current.error).toBe("无法连接到服务端（同源），请确认服务已启动");
    expect(result.current.messages[1].status).toBe("interrupted");
    expect(result.current.streaming).toBe(false);
  });

  it("创建会话失败时不留下空消息，只给错误提示", async () => {
    api.sessionCreate.mockRejectedValue(
      new ApiRequestError("network", "无法连接到服务端（同源），请确认服务已启动"),
    );
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("你好");
    });

    expect(result.current.messages).toEqual([]);
    expect(result.current.error).toBe("无法连接到服务端（同源），请确认服务已启动");
    expect(result.current.streaming).toBe(false);
  });
});
