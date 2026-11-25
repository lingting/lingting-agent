/**
 * 工具事件用例（P3-030 的 `tool.web`）：`useChat` 的三个工具分支、确认状态机与 `ToolCard` 的按钮。
 *
 * 口径与 `tool.events` / `tool.tui` 一致：只断言事件消费函数的行为（卡片状态、上行次数、
 * 未完成标记），不做界面快照。
 */

import type { ClientDecision, MessageSendVO, Run, RunEvent, SessionSummary } from "@lingting/agent-api";
import { act, cleanup, render, renderHook, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { agentApi, type LinkListener } from "@/api";
import { type ChatItem, type ToolCard } from "@/pages/chat/chatMessage";
import { ToolCardView } from "@/pages/chat/ToolCard";
import { useChat } from "@/pages/chat/useChat";

/** 最近一次连接监听：用例用它推送下行事件。 */
let push: LinkListener | null = null;

/** 占用调用。 */
const occupy = vi.fn(async (_sessionId: string) => undefined);

/** 确认上行调用。 */
const confirm = vi.fn(async (_toolCallId: string, _decision: ClientDecision): Promise<void> => undefined);

vi.mock("@/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api")>();
  return {
    ...actual,
    agentApi: { messageCreate: vi.fn(), runCancel: vi.fn(), sessionCreate: vi.fn() },
    ChatLink: class {
      constructor(_client: unknown, listener: LinkListener) {
        push = listener;
      }
      async connect() {
        return "c-1";
      }
      occupy = occupy;
      confirm = confirm;
      async release() {
        push = null;
      }
    },
  };
});

const api = agentApi as unknown as {
  messageCreate: ReturnType<typeof vi.fn>;
  runCancel: ReturnType<typeof vi.fn>;
  sessionCreate: ReturnType<typeof vi.fn>;
};

/** 会话摘要。 */
function session(): SessionSummary {
  return {
    id: "s-1",
    messageCount: "0",
    createdAtMs: "1767225600000",
    model: "mock-model",
    title: "工具会话",
    updatedAtMs: "1767225600000",
  };
}

/** 一次 Run。 */
function run(): Run {
  return {
    createdAtMs: "1767225600000",
    error: null,
    finishedAtMs: null,
    id: "r-1",
    inputTokens: null,
    model: "mock-model",
    outputTokens: null,
    sessionId: "s-1",
    status: "QUEUED",
    updatedAtMs: "1767225600000",
  };
}

/** 发送消息的响应。 */
function sent(): MessageSendVO {
  return {
    message: {
      content: "你好",
      createdAtMs: "1767225600000",
      id: "m-1",
      inputTokens: null,
      model: null,
      outputTokens: null,
      role: "USER",
      runId: "r-1",
      sessionId: "s-1",
    },
    run: run(),
  };
}

/** 一条事件；`extra` 覆盖工具字段。 */
function event(kind: RunEvent["event"], extra: Partial<RunEvent> = {}): RunEvent {
  return {
    createdAtMs: "1767225600000",
    event: kind,
    runId: "r-1",
    seq: "0",
    sessionId: "s-1",
    ...extra,
  };
}

/** 把事件推给状态机。 */
async function emit(...events: RunEvent[]) {
  await act(async () => {
    for (const item of events) push?.({ type: "run", event: item });
  });
}

/** 本次对话里的工具卡片（按到达顺序）。 */
function cards(items: ChatItem[]): ToolCard[] {
  return items.filter((item): item is ToolCard => item.kind === "tool");
}

beforeEach(() => {
  vi.clearAllMocks();
  push = null;
  api.sessionCreate.mockResolvedValue(session());
  api.messageCreate.mockResolvedValue(sent());
  api.runCancel.mockResolvedValue(run());
  confirm.mockImplementation(async () => undefined);
});

afterEach(cleanup);

describe("useChat 工具事件", () => {
  it("三个事件按 toolCallId 配对成一张卡片", async () => {
    const { result } = renderHook(() => useChat());
    await act(async () => {
      await result.current.send("算一下");
    });

    await emit(event("tool_call_started", { toolCallId: "c-1", toolName: "calculator" }));
    let card = cards(result.current.items)[0];
    expect(card.status).toBe("running");
    expect(card.name).toBe("calculator");

    // 开始事件已建卡片：确认事件改写同一条，不再追加第二张。
    await emit(
      event("tool_call_confirm_required", {
        toolArguments: '{"url":"http://127.0.0.1:1/"}',
        toolCallId: "c-1",
        toolName: "calculator",
      }),
    );
    expect(cards(result.current.items)).toHaveLength(1);
    card = cards(result.current.items)[0];
    expect(card.status).toBe("confirm");
    expect(card.arguments).toBe('{"url":"http://127.0.0.1:1/"}');

    await emit(
      event("tool_call_completed", { toolCallId: "c-1", toolDurationMs: "12", toolStatus: "OK" }),
    );
    card = cards(result.current.items)[0];
    expect(card.status).toBe("ok");
    expect(card.durationMs).toBe("12");
    expect(card.note).toBeNull();
  });

  it("未授权的结果事件带服务端文案，且不把失败当成功", async () => {
    const { result } = renderHook(() => useChat());
    await act(async () => {
      await result.current.send("调网络");
    });

    await emit(
      event("tool_call_completed", {
        error: "工具未授权：http_request",
        toolCallId: "c-2",
        toolName: "http_request",
        toolStatus: "DENIED",
      }),
    );
    const card = cards(result.current.items)[0];
    expect(card.status).toBe("failed");
    expect(card.note).toBe("工具未授权：http_request");
  });

  it("确认上行失败时卡片退回可确认态", async () => {
    confirm.mockImplementation(async () => {
      throw new Error("客户端未注册");
    });
    const { result } = renderHook(() => useChat());
    await act(async () => {
      await result.current.send("确认");
    });
    await emit(
      event("tool_call_confirm_required", { toolCallId: "c-3", toolName: "http_request" }),
    );

    await act(async () => {
      await result.current.confirm("c-3", "APPROVE");
    });
    const card = cards(result.current.items)[0];
    expect(card.status).toBe("confirm");
    expect(result.current.error).not.toBeNull();
  });

  it("确认在途时重复点击只上行一次", async () => {
    let release: (() => void) | null = null;
    confirm.mockImplementation(
      () =>
        new Promise<void>((resolveReady) => {
          release = resolveReady;
        }),
    );
    const { result } = renderHook(() => useChat());
    await act(async () => {
      await result.current.send("确认");
    });
    await emit(
      event("tool_call_confirm_required", { toolCallId: "c-4", toolName: "http_request" }),
    );

    await act(async () => {
      void result.current.confirm("c-4", "APPROVE");
      void result.current.confirm("c-4", "REJECT");
    });
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(cards(result.current.items)[0].status).toBe("submitted");

    await act(async () => {
      release?.();
    });
  });

  it("Run 结束时仍停在确认态的卡片标未完成", async () => {
    const { result } = renderHook(() => useChat());
    await act(async () => {
      await result.current.send("确认");
    });
    await emit(
      event("tool_call_confirm_required", { toolCallId: "c-5", toolName: "http_request" }),
      event("run_completed"),
    );

    const card = cards(result.current.items)[0];
    expect(card.status).toBe("unfinished");
    expect(card.note).toBe("Run 已结束，未收到结果");
  });
});

describe("ToolCardView", () => {
  /** 一张待确认卡片。 */
  function card(status: ToolCard["status"]): ToolCard {
    return {
      arguments: '{"path":"a.txt"}',
      durationMs: null,
      key: "c-9",
      kind: "tool",
      name: "file_read",
      note: null,
      status,
    };
  }

  it("确认态两个按钮可点，点击回调带决定", () => {
    const onConfirm = vi.fn();
    render(<ToolCardView card={card("confirm")} onConfirm={onConfirm} />);

    const approve = screen.getByRole("button", { name: /允\s*许/ }) as HTMLButtonElement;
    const reject = screen.getByRole("button", { name: /拒\s*绝/ }) as HTMLButtonElement;
    expect(approve.disabled).toBe(false);

    reject.click();
    expect(onConfirm).toHaveBeenCalledWith("c-9", "REJECT");
  });

  it("已提交后按钮不可再点", () => {
    render(<ToolCardView card={card("submitted")} onConfirm={() => undefined} />);

    const approve = screen.getByRole("button", { name: /允\s*许/ }) as HTMLButtonElement;
    const reject = screen.getByRole("button", { name: /拒\s*绝/ }) as HTMLButtonElement;
    expect(approve.disabled).toBe(true);
    expect(reject.disabled).toBe(true);
    expect(screen.getByText("等结果")).toBeTruthy();
  });
});
