/** SSE 切帧用例：分块到达、CRLF、多帧粘包与非法数据。 */

import { describe, expect, it } from "vite-plus/test";

import type { RunEvent } from "@lingting/agent-api";

import type { AgentApiClient } from "@/api";
import { ApiRequestError, subscribeRunEventStream } from "@/api";

/** 一帧事件的 JSON 载荷（只保留断言用到的字段）。 */
function eventData(kind: string, seq: number, delta: string | null = null): string {
  return JSON.stringify({
    event: kind,
    seq: String(seq),
    delta,
    runId: "r-1",
    sessionId: "s-1",
    createdAtMs: "1767225600000",
  });
}

/** 由 `data:` 行拼一帧。 */
function frame(kind: string, seq: number, delta: string | null = null): string {
  return `id: ${seq}\nevent: ${kind}\ndata: ${eventData(kind, seq, delta)}\n\n`;
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
  const events: RunEvent[] = [];
  for await (const event of subscribeRunEventStream(client, { runId: "r-1" })) {
    events.push(event);
  }
  return events;
}

describe("subscribeRunEventStream", () => {
  it("按空行切帧并忽略 id / event 行", async () => {
    const events = await collect([frame("run_started", 0), frame("message_delta", 1, "你好")]);

    expect(events.map((event) => event.event)).toEqual(["run_started", "message_delta"]);
    expect(events[1].delta).toBe("你好");
  });

  it("一帧跨多个 chunk 时能拼起来", async () => {
    const text = frame("run_completed", 2);
    const chunks = [text.slice(0, 12), text.slice(12, 30), text.slice(30)];

    const events = await collect(chunks);

    expect(events).toHaveLength(1);
    expect(events[0].event).toBe("run_completed");
  });

  it("多个帧粘在一个 chunk 里时逐条产出", async () => {
    const events = await collect([frame("message_delta", 1, "a") + frame("message_delta", 2, "b")]);

    expect(events).toHaveLength(2);
    expect(events.map((event) => event.event)).toEqual(["message_delta", "message_delta"]);
  });

  it("兼容 CRLF 帧分隔", async () => {
    const text = `id: 0\r\nevent: run_started\r\ndata: ${eventData("run_started", 0)}\r\n\r\n`;

    const events = await collect([text]);

    expect(events).toHaveLength(1);
    expect(events[0].event).toBe("run_started");
  });

  it("data 不是合法 JSON 时报 decode 错误", async () => {
    const text = "event: message_delta\ndata: 不是 JSON\n\n";

    await expect(collect([text])).rejects.toThrow(ApiRequestError);
    await expect(collect([text])).rejects.toMatchObject({ kind: "decode" });
  });

  it("没有 data 行的帧被跳过", async () => {
    const events = await collect(["event: heartbeat\n\n", frame("run_completed", 3)]);

    expect(events.map((event) => event.event)).toEqual(["run_completed"]);
  });
});
