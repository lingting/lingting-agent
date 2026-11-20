import { ApiDefinitions, type ClientEvent, type ClientStreamParams } from "@lingting/agent-api";

import type { AgentApiClient } from "./client";
import { ApiRequestError } from "./errors";

/** SSE 帧分隔：空行；兼容 CRLF。 */
const FRAME_END = /\r?\n\r?\n/;

/**
 * 订阅客户端下行事件流（`api/client/stream`，D-P1.1-001）。
 *
 * 不用原生 `EventSource`：它拿不到建立失败的 4xx 错误体，断线会自动重连，也无法中断；
 * 因此走 fetch 的流式响应自行切帧。path 取自生成的 `ApiDefinitions.clientStream.path`，
 * 代码中不出现 path 字面量。
 *
 * 帧格式：`id:` / `event:` / `data:` 三行加空行，事件类型与字段全在 `data` 的 JSON 里；
 * 没有 `data:` 的帧是保活注释（`: keepalive`），直接丢弃。
 */
export async function* subscribeClientEventStream(
  client: AgentApiClient,
  params: ClientStreamParams,
  signal?: AbortSignal,
): AsyncGenerator<ClientEvent> {
  const response = await client.openStream(ApiDefinitions.clientStream.path, params, signal);
  const body = response.body;
  if (!body) throw new ApiRequestError("decode", "事件流没有响应体");

  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  try {
    for (;;) {
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

/** 解析一帧：拼接 `data:` 行后反序列化成生成的事件类型；注释帧返回 `null`。 */
function parseFrame(frame: string): ClientEvent | null {
  const data = frame
    .split(/\r?\n/)
    .filter((line) => line.startsWith("data:"))
    .map((line) => line.slice("data:".length).replace(/^ /, ""))
    .join("\n");
  if (!data) return null;

  try {
    return JSON.parse(data) as ClientEvent;
  } catch {
    throw new ApiRequestError("decode", "事件数据不是有效的 JSON");
  }
}
