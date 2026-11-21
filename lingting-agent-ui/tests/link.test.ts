/** 连接用例：注册、占用、心跳与卸载收尾（D-P1.1-001、RT-020）。 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import type { ClientEvent } from "@lingting/agent-api";

import type { AgentApiClient } from "@/api";
import { ChatLink, NOT_REGISTERED } from "@/api";

/** 一帧下行事件。 */
function frame(event: ClientEvent): string {
  return `id: 0\nevent: ${event.event}\ndata: ${JSON.stringify(event)}\n\n`;
}

/** 一条下线事件（只保留断言用到的字段）。 */
function clientEvent(
  event: ClientEvent["event"],
  closeReason: ClientEvent["closeReason"] = null,
): ClientEvent {
  return {
    closeReason,
    createdAtMs: "1767225600000",
    event,
    inquiryAtMs: null,
    inquiryDeadlineMs: null,
    seq: "0",
    sessionEvent: null,
  };
}

/** 把若干文本块包成流式响应；`close = false` 时保持连接不结束。 */
function streamResponse(chunks: readonly string[], close = true): Response {
  const encoder = new TextEncoder();
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
      if (close) controller.close();
    },
  });
  return new Response(body, { headers: { "content-type": "text/event-stream" } });
}

/** 假客户端：记录调用顺序，事件流内容由用例给定。 */
function fakeClient(chunks: readonly string[] = []) {
  const calls: string[] = [];
  const client = {
    clientCreate: async () => {
      calls.push("clientCreate");
      return { clientId: "c-1", pongAtMs: "1" };
    },
    clientUpdate: async (po: { kind: string }) => {
      calls.push(`clientUpdate:${po.kind}`);
      return { pongAtMs: "2" };
    },
    clientDelete: async () => {
      calls.push("clientDelete");
    },
    occupancyCreate: async () => {
      calls.push("occupancyCreate");
      return {
        acquiredAtMs: "1",
        clientId: "c-1",
        runtimeSessionId: "rs-1",
        sessionId: "s-1",
      };
    },
    occupancyDelete: async () => {
      calls.push("occupancyDelete");
    },
    // 没有给定帧时保持连接不结束：否则断流会把心跳一起停掉。
    openStream: async () => streamResponse(chunks, chunks.length > 0),
  } as unknown as AgentApiClient;
  return { calls, client };
}

/** 等一轮微任务，让后台的下行流消费任务先跑起来。 */
async function settle() {
  await Promise.resolve();
  await Promise.resolve();
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("ChatLink", () => {
  it("注册后建立下行流并启动心跳", async () => {
    const { calls, client } = fakeClient();
    const link = new ChatLink(client, () => undefined);

    const clientId = await link.connect();
    expect(clientId).toBe("c-1");
    expect(calls).toEqual(["clientCreate"]);

    // 心跳每 15 秒一拍，与 service-core 的 PING_INTERVAL 一致。
    await vi.advanceTimersByTimeAsync(15_000);
    expect(calls).toContain("clientUpdate:PING");
  });

  it("换会话时先交还旧占用再占用新会话", async () => {
    const { calls, client } = fakeClient();
    const link = new ChatLink(client, () => undefined);
    await link.connect();

    await link.occupy("s-1");
    await link.occupy("s-2");

    expect(calls.filter((call) => call.startsWith("occupancy"))).toEqual([
      "occupancyCreate",
      "occupancyDelete",
      "occupancyCreate",
    ]);
  });

  it("卸载时先交还占用、再反注册，并停掉心跳", async () => {
    const { calls, client } = fakeClient();
    const link = new ChatLink(client, () => undefined);
    await link.connect();
    await link.occupy("s-1");

    await link.release();
    expect(calls.slice(-2)).toEqual(["occupancyDelete", "clientDelete"]);

    // 收尾之后心跳不再发请求。
    const before = calls.length;
    await vi.advanceTimersByTimeAsync(30_000);
    expect(calls).toHaveLength(before);
  });

  it("下行流推 session_event 时交给监听器", async () => {
    const event: ClientEvent = {
      ...clientEvent("session_event"),
      sessionEvent: {
        createdAtMs: "1767225600000",
        delta: "你好",
        error: null,
        event: "message_delta",
        inputTokens: null,
        messageId: null,
        outputTokens: null,
        runId: "r-1",
        seq: "1",
        sessionId: "s-1",
        status: null,
      },
    };
    const { client } = fakeClient([frame(event)]);
    const received: string[] = [];
    const link = new ChatLink(client, (linkEvent) => {
      if (linkEvent.type === "run") received.push(linkEvent.event.delta ?? "");
    });

    await link.connect();
    await settle();

    expect(received).toEqual(["你好"]);
  });

  it("client_closed 帧把连接判为断并给出原因", async () => {
    const { client } = fakeClient([frame(clientEvent("client_closed", "SERVER_SHUTDOWN"))]);
    const reasons: string[] = [];
    const link = new ChatLink(client, (linkEvent) => {
      if (linkEvent.type === "closed") reasons.push(linkEvent.reason);
    });

    await link.connect();
    await settle();

    expect(reasons).toEqual(["服务正在关闭"]);
  });

  it("心跳收到「客户端未注册」时把连接判为断", async () => {
    const calls: string[] = [];
    const client = {
      clientCreate: async () => ({ clientId: "c-1", pongAtMs: "1" }),
      clientUpdate: async () => {
        throw new Error(NOT_REGISTERED);
      },
      clientDelete: async () => undefined,
      occupancyCreate: async () => undefined,
      occupancyDelete: async () => undefined,
      openStream: async () => streamResponse([], false),
      calls,
    } as unknown as AgentApiClient;
    const reasons: string[] = [];
    const link = new ChatLink(client, (linkEvent) => {
      if (linkEvent.type === "closed") reasons.push(linkEvent.reason);
    });

    await link.connect();
    await vi.advanceTimersByTimeAsync(15_000);

    expect(reasons).toEqual([NOT_REGISTERED]);
  });
});
