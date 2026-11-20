/** 下行流切帧用例：分块到达、CRLF、多帧粘包、保活注释帧与非法数据（D-P1.1-001）。 */

import { describe, expect, it } from "vite-plus/test";

import type { ClientEvent, RunEvent } from "@lingting/agent-api";

import type { AgentApiClient } from "@/api";
import { ApiRequestError, subscribeClientEventStream } from "@/api";

/** 一条 session_event 的 RunEvent 载荷（只保留断言用到的字段）。 */
function runEvent(deltas: string): RunEvent {
  return {
    createdAtMs: "1767225600000",
    delta: deltas,
    error: null,
    event: "message_delta",
    inputTokens: null,
    messageId: null,
    outputTokens: null,
    runId: "r-1",
    seq: "1",
    sessionId: "s-1",
    status: null,
  };
}

/** 一条下行事件的 JSON（只保留断言用到的字段）。 */
function eventData(kind: ClientEvent["event"], delta: string | null = null): string {
  return JSON.stringify({
    event: kind,
    seq: "0",
    createdAtMs: "1767225600000",
    sessionEvent: delta === null ? null : runEvent(delta),
    inquiryAtMs: null,
    inquiryDeadlineMs: null,
    closeReason: null,
  });
}

/** 由 `data:` 行拼一帧。 */
function frame(kind: ClientEvent["event"], delta: string | null = null): string {
  return `id: 0\nevent: ${kind}\ndata: ${eventData(kind, delta)}\n\n`;
}

/** 把若干文本块包成流式响应（每个块是一次 chunk 到达）。 */
function streamResponse(chunks: readonly string[]): Response {
  const encoder = new TextEncoder();
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) {
        controller.enqueue(encoder.encode(chunk));
      }
      controller.close();
    },
  });
  return new Response(body, { headers: { "content-type": "text/event-stream" } });
}

/** 用一个假的客户端跑事件流，返回收到的事件。 */
async function collect(chunks: readonly string[]) {
  const client = { openStream: async () => streamResponse(chunks) } as unknown as AgentApiClient;
  const events: ClientEvent[] = [];
  for await (const event of subscribeClientEventStream(client, { clientId: "c-1" })) {
    events.push(event);
  }
  return events;
}

describe("subscribeClientEventStream", () => {
  it("按空行切帧并忽略 id / event 行", async () => {
    const events = await collect([frame("session_event", "你好")]);

    expect(events).toHaveLength(1);
    expect(events[0].event).toBe("session_event");
    expect(events[0].sessionEvent?.delta).toBe("你好");
  });

  it("一帧跨多个 chunk 时能拼起来", async () => {
    const text = frame("session_event");
    const chunks = [text.slice(0, 12), text.slice(12, 30), text.slice(30)];

    const events = await collect(chunks);

    expect(events).toHaveLength(1);
    expect(events[0].event).toBe("session_event");
  });

  it("多个帧粘在一个 chunk 里时逐条产出", async () => {
    const events = await collect([frame("session_event", "a") + frame("session_event", "b")]);

    expect(events).toHaveLength(2);
    expect(events.map((event) => event.event)).toEqual(["session_event", "session_event"]);
  });

  it("兼容 CRLF 帧分隔", async () => {
    const text = `id: 0\r\nevent: inquiry\r\ndata: ${eventData("inquiry")}\r\n\r\n`;

    const events = await collect([text]);

    expect(events).toHaveLength(1);
    expect(events[0].event).toBe("inquiry");
  });

  it("data 不是合法 JSON 时报 decode 错误", async () => {
    const text = "event: session_event\ndata: 不是 JSON\n\n";

    await expect(collect([text])).rejects.toThrow(ApiRequestError);
    await expect(collect([text])).rejects.toMatchObject({ kind: "decode" });
  });

  it("保活注释帧被跳过", async () => {
    const events = await collect([": keepalive\n\n", frame("client_closed")]);

    expect(events).toHaveLength(1);
    expect(events[0].event).toBe("client_closed");
  });
});
