import { ApiDefinitions, type RunEvent, type RunStreamParams } from "@lingting/agent-api";

import type { AgentApiClient } from "./client";
import { ApiRequestError } from "./errors";

/** SSE 帧分隔：空行；兼容 CRLF。 */
const FRAME_END = /\r?\n\r?\n/;

/**
 * 订阅 Run 的事件流。
 *
 * 不用原生 `EventSource`：它无法在订阅前拿到 4xx 错误体，还会在断开后自动重连（重复订阅同一 Run
 * 会被服务端拒绝），也无法中断。因此走 fetch 的流式响应自行解析帧（D-007）；path 取自生成的
 * `ApiDefinitions.runStream.path`，代码中不出现 path 字面量。
 *
 * 事件格式见 docs/api/session.md「SSE 事件流」：`id:` / `event:` / `data:` 三行加空行，
 * 事件类型与字段全在 `data` 的 JSON 里，`id:` 与 `event:` 行不参与解析。
 */
export async function* subscribeRunEventStream(
  client: AgentApiClient,
  params: RunStreamParams,
  signal?: AbortSignal,
): AsyncGenerator<RunEvent> {
  const response = await client.openStream(ApiDefinitions.runStream.path, params, signal);
  const body = response.body;
  if (!body) throw new ApiRequestError("decode", "事件流没有响应体");

  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) return;
      buffer += decoder.decode(value, { stream: true });
      for (;;) {
        const match = FRAME_END.exec(buffer);
        if (!match) break;
        const frame = buffer.slice(0, match.index);
        buffer = buffer.slice(match.index + match[0].length);
        const event = parseFrame(frame);
        if (event) yield event;
      }
    }
  } finally {
    await reader.cancel().catch(() => undefined);
  }
}

/** 解析一帧：拼接 `data:` 行后反序列化成生成的事件类型。 */
function parseFrame(frame: string): RunEvent | null {
  const data = frame
    .split(/\r?\n/)
    .filter((line) => line.startsWith("data:"))
    .map((line) => line.slice("data:".length).replace(/^ /, ""))
    .join("\n");
  if (!data) return null;

  try {
    return JSON.parse(data) as RunEvent;
  } catch {
    throw new ApiRequestError("decode", "事件数据不是有效的 JSON");
  }
}
