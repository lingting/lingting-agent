/**
 * 中断态与双 Run 归类（P6A-015 / 验收 I-5）。
 *
 * 三件事在这里取证：
 *
 * - 提示文案只有一个产点（`pausedNoticeOf`），且只说明「怎么继续」，不带恢复动作；
 * - 对话页把中断态渲染成提示条，输入区照旧可用；
 * - 一次发送同时带出「被恢复的旧 Run」与「本次的新 Run」时，两段回复按事件里的 `runId` 分开，
 *   不按到达顺序猜。
 */

import type { Message, MessageSendVO, Run, RunEvent, SessionSummary } from "@lingting/agent-api";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { agentApi, type LinkListener } from "@/api";
import { pausedNoticeOf } from "@/pages/chat/chatMessage";
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
    title: "中断会话",
    updatedAtMs: "1767225600000",
  };
}

/** 一条消息。 */
function message(id: string, role: Message["role"], runId: string): Message {
  return {
    content: `${role} ${id}`,
    createdAtMs: "1767225600000",
    id,
    inputTokens: null,
    model: null,
    outputTokens: null,
    role,
    runId,
    sessionId: "s-1",
  };
}

/** 一次 Run。 */
function run(id: string, status: Run["status"]): Run {
  return {
    createdAtMs: "1767225600000",
    error: null,
    finishedAtMs: null,
    id,
    inputTokens: null,
    interruptedReason: status === "PAUSED" ? "服务重启，执行中的 Run 已中断" : null,
    lastCheckpointSeq: "3",
    model: "mock-model",
    outputTokens: null,
    resumeCount: "0",
    sessionId: "s-1",
    status,
    updatedAtMs: "1767225600000",
  };
}

/** 发送消息的响应：本次消息产生新 Run `r-new`。 */
function sent(): MessageSendVO {
  return { message: message("m-1", "USER", "r-new"), run: run("r-new", "QUEUED") };
}

/** 一条属于指定 Run 的事件。 */
function event(runId: string, kind: RunEvent["event"], delta: string | null = null): RunEvent {
  return {
    createdAtMs: "1767225600000",
    delta,
    error: null,
    event: kind,
    inputTokens: null,
    messageId: null,
    outputTokens: null,
    runId,
    seq: "0",
    sessionId: "s-1",
    status: null,
  };
}

/** 把若干条 Run 事件当作下行流推给状态机。 */
async function emit(...events: RunEvent[]) {
  await act(async () => {
    for (const item of events) push?.({ type: "run", event: item });
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  push = null;
  window.sessionStorage.clear();
  api.sessionCreate.mockResolvedValue(session());
  api.messageCreate.mockResolvedValue(sent());
  api.messageList.mockResolvedValue([]);
  api.runGet.mockResolvedValue(run("r-old", "PAUSED"));
});

describe("pausedNoticeOf", () => {
  it("文案只说怎么继续，不含恢复动作", () => {
    const text = pausedNoticeOf("服务重启，执行中的 Run 已中断");

    expect(text).toBe("已中断（发送消息可继续）：服务重启，执行中的 Run 已中断");
    // 客户端不提供恢复动作（D-P6-009）：文案里不能出现「恢复」按钮式的措辞。
    expect(text).not.toContain("立即恢复");
  });
});

describe("中断态", () => {
  it("刷新后看到中断态：显示原因并提示发送消息即可继续", async () => {
    window.sessionStorage.setItem("lingting.session", "s-1");
    api.messageList.mockResolvedValue([message("m-1", "USER", "r-old")]);
    const { result } = renderHook(() => useChat());

    await waitFor(() => expect(result.current.paused).not.toBeNull());
    expect(result.current.paused?.runId).toBe("r-old");
    expect(result.current.paused?.reason).toBe("服务重启，执行中的 Run 已中断");
    // 中断态不影响输入：这不是忙碌状态。
    expect(result.current.streaming).toBe(false);
  });

  it("发一条消息后中断提示消失（恢复由消息触发）", async () => {
    window.sessionStorage.setItem("lingting.session", "s-1");
    api.messageList.mockResolvedValue([message("m-1", "USER", "r-old")]);
    const { result } = renderHook(() => useChat());
    await waitFor(() => expect(result.current.paused).not.toBeNull());

    await act(async () => {
      await result.current.send("继续");
    });

    expect(result.current.paused).toBeNull();
    // 被恢复的旧 Run 开始执行时同样清掉提示，不需要额外动作。
    await emit(event("r-old", "run_started"), event("r-old", "message_delta", "接着跑"));
    expect(result.current.paused).toBeNull();
  });
});

describe("双 Run 归类", () => {
  it("一次发送的两条 Run 各归各的助手气泡", async () => {
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("继续");
    });
    // 服务端先恢复旧 Run（`r-old`），再执行本次的新 Run（`r-new`）。
    await emit(
      event("r-old", "run_started"),
      event("r-old", "message_delta", "旧"),
      event("r-old", "run_completed"),
      event("r-new", "run_started"),
      event("r-new", "message_delta", "新"),
      event("r-new", "run_completed"),
    );

    const assistants = result.current.messages.filter((item) => item.role === "ASSISTANT");
    expect(assistants.map((item) => item.content)).toEqual(["旧", "新"]);
    expect(assistants.every((item) => item.status === "done")).toBe(true);
    await waitFor(() => expect(result.current.streaming).toBe(false));
  });

  it("被恢复的旧 Run 先结束时，界面仍在等新 Run", async () => {
    const { result } = renderHook(() => useChat());

    await act(async () => {
      await result.current.send("继续");
    });
    await emit(event("r-old", "run_started"), event("r-old", "run_completed"));

    // 旧 Run 结束不等于本轮结束：新 Run 还没开始执行。
    expect(result.current.streaming).toBe(true);

    await emit(event("r-new", "run_started"), event("r-new", "run_completed"));
    await waitFor(() => expect(result.current.streaming).toBe(false));
  });
});
