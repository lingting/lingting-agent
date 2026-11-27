/**
 * 对话状态机用例：注册连接、占用会话、累计增量、停止生成、连接中断与失败分支。
 *
 * 下行事件只有一个来源（`api/client/stream`，D-P1.1-004），因此这里 mock 连接对象
 * `ChatLink`，用它的监听器把事件推给状态机。
 */

import type { Message, MessageSendVO, Run, RunEvent, SessionSummary } from "@lingting/agent-api";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { ApiRequestError, agentApi, type LinkListener } from "@/api";
import { useChat } from "@/pages/chat/useChat";

/** 最近一次创建出来的连接监听的推送函数。 */
let push: LinkListener | null = null;

/** 连接上的占用调用。 */
const occupy = vi.fn(async (_sessionId: string) => undefined);

vi.mock("@/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api")>();
  return {
    ...actual,
    agentApi: {
      messageCreate: vi.fn(),
      messageList: vi.fn(),
      runCancel: vi.fn(),
      runGet: vi.fn(),
      sessionCreate: vi.fn(),
    },
    ChatLink: class {
      constructor(_client: unknown, listener: LinkListener) {
        push = listener;
      }
      async connect() {
        return "c-1";
      }
      occupy = occupy;
      async release() {
        push = null;
      }
    },
  };
});

const api = agentApi as unknown as {
  messageCreate: ReturnType<typeof vi.fn>;
  messageList: ReturnType<typeof vi.fn>;
  runCancel: ReturnType<typeof vi.fn>;
  runGet: ReturnType<typeof vi.fn>;
  sessionCreate: ReturnType<typeof vi.fn>;
};

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
    interruptedReason: null,
    lastCheckpointSeq: null,
    model: "mock-model",
    outputTokens: null,
    resumeCount: "0",
    sessionId: "s-1",
    status,
    updatedAtMs: "1767225600000",
  };
}

/** 发送消息的响应。 */
function sent(): MessageSendVO {
  return { message: userMessage("你好"), run: run("QUEUED") };
}

/** 一条 Run 事件。 */
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

/** 把一条 Run 事件当作下行流推给状态机。 */
async function emit(...events: RunEvent[]) {
  await act(async () => {
    for (const item of events) push?.({ type: "run", event: item });
  });
}

/** 把连接关闭推给状态机。 */
async function close(reason: string) {
  await act(async () => {
    push?.({ type: "closed", reason });
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  push = null;
  // 会话 ID 会落到 sessionStorage（刷新后接着看）：每个用例从干净状态起步。
  window.sessionStorage.clear();
  api.sessionCreate.mockResolvedValue(session());
  api.messageCreate.mockResolvedValue(sent());
  api.messageList.mockResolvedValue([]);
  api.runGet.mockResolvedValue(run("PAUSED"));
  api.runCancel.mockResolvedValue(run("CANCELLED"));
});

describe("useChat", () => {
  it("首次发送先占用会话，追加用户消息并累计流式增量", async () => {
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("  你好  ");
    });

    expect(api.sessionCreate).toHaveBeenCalledWith({ title: "你好" });
    expect(occupy).toHaveBeenCalledWith("s-1");
    expect(api.messageCreate).toHaveBeenCalledWith({ content: "你好", sessionId: "s-1" });
    expect(result.current.messages.map((message) => message.role)).toEqual(["USER", "ASSISTANT"]);

    await emit(event("run_started"), event("message_delta", "你"), event("message_delta", "好"));
    expect(result.current.messages[1].content).toBe("你好");

    await emit(event("run_completed"));
    expect(result.current.messages[1].status).toBe("done");
    expect(result.current.error).toBeNull();
    await waitFor(() => expect(result.current.streaming).toBe(false));
  });

  it("发送响应返回前到达的事件先缓冲，再按序放出", async () => {
    api.messageCreate.mockImplementation(async () => {
      // 服务端可能比响应更快地推出 run_started：这里模拟「先到事件、后拿到 Run ID」。
      await emit(event("run_started"), event("message_delta", "半"));
      return sent();
    });
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("你好");
    });

    expect(result.current.messages.map((message) => message.role)).toEqual(["USER", "ASSISTANT"]);
    expect(result.current.messages[1].content).toBe("半");
  });

  it("第二次发送复用已创建的会话", async () => {
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("第一条");
    });
    await emit(event("run_completed"));
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

  it("停止生成：先取消 Run 再本地收尾，并给回复打标", async () => {
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("你好");
    });
    await emit(event("run_started"), event("message_delta", "半句"));

    await act(async () => {
      await result.current.cancel();
    });

    expect(api.runCancel).toHaveBeenCalledWith({ runId: "r-1" });
    await waitFor(() => expect(result.current.streaming).toBe(false));
    expect(result.current.messages[1].content).toBe("半句（已停止生成）");
    expect(result.current.messages[1].status).toBe("interrupted");
    expect(result.current.error).toBeNull();
  });

  it("执行失败：脱敏原因挂在回复末尾，同时作为提示条错误", async () => {
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("你好");
    });
    await emit(
      event("run_started"),
      event("message_delta", "半句"),
      event("run_failed", null, "调用模型失败：认证失败"),
    );

    expect(result.current.messages[1].status).toBe("interrupted");
    expect(result.current.messages[1].content).toBe("半句（执行失败：调用模型失败：认证失败）");
    expect(result.current.error).toBe("调用模型失败：认证失败");
    await waitFor(() => expect(result.current.streaming).toBe(false));
  });

  it("服务关闭事件把回复标成中断并给出原因", async () => {
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("你好");
    });
    await emit(
      event("run_started"),
      event("message_delta", "半句"),
      event("server_shutdown", null, "服务正在关闭，事件流已终止"),
    );

    expect(result.current.messages[1].status).toBe("interrupted");
    expect(result.current.messages[1].content).toBe("半句（连接中断，内容可能不完整）");
    expect(result.current.error).toBe("服务正在关闭，事件流已终止");
  });

  it("连接中断时保留已收内容并展示可读错误", async () => {
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("你好");
    });
    await emit(event("run_started"), event("message_delta", "半句"));
    await close("无法连接到服务端（同源），请确认服务已启动");

    expect(result.current.error).toBe("连接中断：无法连接到服务端（同源），请确认服务已启动");
    expect(result.current.messages[1].status).toBe("interrupted");
    await waitFor(() => expect(result.current.streaming).toBe(false));
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
