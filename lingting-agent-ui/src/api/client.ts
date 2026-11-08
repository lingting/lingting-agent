import { ApiClient, type ApiError } from "@lingting/agent-api";

import { ApiRequestError, isAborted } from "./errors";

/** 服务端统一响应包装 `R<T>`（见 docs/api/readme.md「响应形态」）。 */
type ApiEnvelope<T> = { code: number; message: string; data?: T | null };

/** 一次请求的可选项。 */
type RequestOptions = {
  body?: unknown;
  query?: unknown;
  accept?: string;
  signal?: AbortSignal;
};

/**
 * 走 `fetch` 的 SDK Transport。
 *
 * 分工：生成的 `ApiClient` 负责 method / path / 参数位置，这里只负责拼地址、发请求、判定状态码、
 * 拆掉 `R<T>` 包装。界面代码不出现任何 path 字面量（P1-016 验收标准）。
 */
export class AgentApiClient extends ApiClient {
  /**
   * 服务基地址，留空表示同源。
   *
   * 开发期由 Vite 代理把 `/api/` 转发到 `127.0.0.1:20102`，发布态由 Rust 直接托管静态资源与 API，
   * 两种形态都不需要跨域地址（D-022、D-027）。
   */
  private readonly baseUrl: string;

  constructor(baseUrl = "") {
    super();
    this.baseUrl = baseUrl;
  }

  /** 订阅事件流：SSE 不走 `R<T>` 包装，调用方需要原始响应自行解析帧（D-007）。 */
  openStream(path: string, query?: unknown, signal?: AbortSignal): Promise<Response> {
    return this.request("GET", path, { query, accept: "text/event-stream", signal });
  }

  protected async call<T>(
    method: string,
    path: string,
    body?: unknown,
    query?: unknown,
  ): Promise<T> {
    const response = await this.request(method, path, { body, query });
    const payload = await readPayload(response);
    if (!response.ok) throw toFailure(payload, response.status);
    return unwrapEnvelope<T>(payload);
  }

  /** 发一次请求；连不上与中断都转成可展示的错误。 */
  private async request(method: string, path: string, options: RequestOptions): Promise<Response> {
    const headers: Record<string, string> = { accept: options.accept ?? "application/json" };
    const init: RequestInit = { headers, method };
    if (options.body !== undefined) {
      headers["content-type"] = "application/json";
      init.body = JSON.stringify(options.body);
    }
    if (options.signal) init.signal = options.signal;

    try {
      return await fetch(buildUrl(this.baseUrl, path, options.query), init);
    } catch (error) {
      if (isAborted(error)) throw new ApiRequestError("aborted", "请求已中断");
      throw new ApiRequestError(
        "network",
        `无法连接到服务端（${this.baseUrl || "同源"}），请确认服务已启动`,
      );
    }
  }
}

/** 拼接接口地址：path 来自生成的 SDK，查询参数按 camelCase 字段名展开。 */
function buildUrl(baseUrl: string, path: string, query?: unknown): string {
  const base = baseUrl || window.location.origin;
  const url = new URL(path.replace(/^\/+/, ""), base);
  for (const [key, value] of Object.entries(asRecord(query))) {
    if (value === undefined || value === null) continue;
    url.searchParams.set(key, String(value));
  }
  return url.toString();
}

/** 查询参数形如 DTO，非对象时视为没有参数。 */
function asRecord(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
}

/** 读取响应体：`Void` 接口是 204 空体，其余接口都是 JSON。 */
async function readPayload(response: Response): Promise<unknown> {
  const text = await response.text();
  if (!text) return undefined;
  try {
    return JSON.parse(text) as unknown;
  } catch {
    throw new ApiRequestError(
      "decode",
      `服务端响应不是有效的 JSON（HTTP ${response.status}）`,
      response.status,
    );
  }
}

/** 失败响应统一为 `{"code":<状态码>,"message":"..."}`，直接复用服务端文案。 */
function toFailure(payload: unknown, status: number): ApiRequestError {
  const failure = payload as Partial<ApiError> | undefined;
  const message = failure?.message?.trim() || `请求失败（HTTP ${status}）`;
  return new ApiRequestError("http", message, status);
}

/** 拆掉 `R<T>` 包装；不是包装形态时原样返回。 */
function unwrapEnvelope<T>(payload: unknown): T {
  return isEnvelope(payload) ? ((payload.data ?? undefined) as T) : (payload as T);
}

function isEnvelope(value: unknown): value is ApiEnvelope<unknown> {
  return typeof value === "object" && value !== null && "code" in value && "message" in value;
}
