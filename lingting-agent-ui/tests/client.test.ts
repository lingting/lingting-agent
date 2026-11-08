/** SDK Transport 用例：`R<T>` 解包、错误映射与请求拼装。 */

import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { AgentApiClient, ApiRequestError } from "@/api";

/** 构造一个 JSON 响应。 */
function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    headers: { "content-type": "application/json" },
    status,
  });
}

/** 记录本次 fetch 的调用参数（地址与 init）。 */
function stubFetch(handler: () => Promise<Response>) {
  const fetchMock = vi.fn((_input: RequestInfo | URL, _init?: RequestInit) => handler());
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("AgentApiClient", () => {
  it("拆掉 R<T> 包装并直接用生成的 path 与 method", async () => {
    const fetchMock = stubFetch(async () =>
      jsonResponse({ code: 200, message: "Success", data: [] }),
    );

    const sessions = await new AgentApiClient().sessionList();

    expect(sessions).toEqual([]);
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toBe(`${window.location.origin}/api/session/list`);
    expect(init?.method).toBe("GET");
  });

  it("GET 的查询参数按 camelCase 字段名拼进查询串", async () => {
    const fetchMock = stubFetch(async () =>
      jsonResponse({ code: 200, message: "Success", data: [] }),
    );

    await new AgentApiClient().messageList({ sessionId: "s-1" });

    const [url] = fetchMock.mock.calls[0];
    expect(String(url)).toBe(`${window.location.origin}/api/message/list?sessionId=s-1`);
  });

  it("POST 把请求体序列化成 JSON 并带上 content-type", async () => {
    const fetchMock = stubFetch(async () =>
      jsonResponse({ code: 200, message: "Success", data: { id: "s-1", messageCount: "0" } }),
    );

    await new AgentApiClient().sessionCreate({ model: null, title: "标题" });

    const [, init] = fetchMock.mock.calls[0];
    expect(init?.method).toBe("POST");
    expect(init?.body).toBe(JSON.stringify({ model: null, title: "标题" }));
    expect((init?.headers as Record<string, string> | undefined)?.["content-type"]).toBe(
      "application/json",
    );
  });

  it("Void 接口的 204 空响应返回 undefined", async () => {
    stubFetch(async () => new Response(null, { status: 204 }));

    await expect(new AgentApiClient().probeVoid()).resolves.toBeUndefined();
  });

  it("非 2xx 复用服务端错误文案与状态码", async () => {
    stubFetch(async () => jsonResponse({ code: 404, message: "会话不存在：s-9" }, 404));

    const client = new AgentApiClient();
    await expect(client.messageList({ sessionId: "s-9" })).rejects.toThrow(ApiRequestError);
    await expect(client.messageList({ sessionId: "s-9" })).rejects.toMatchObject({
      code: 404,
      kind: "http",
      message: "会话不存在：s-9",
    });
  });

  it("连不上服务端时归类为 network 且文案可读", async () => {
    stubFetch(async () => {
      throw new TypeError("fetch failed");
    });

    await expect(new AgentApiClient().sessionList()).rejects.toMatchObject({
      kind: "network",
    });
    await expect(new AgentApiClient().sessionList()).rejects.toThrow(/无法连接到服务端/);
  });

  it("响应不是 JSON 时归类为 decode", async () => {
    stubFetch(async () => new Response("<html>oops</html>", { status: 200 }));

    await expect(new AgentApiClient().sessionList()).rejects.toMatchObject({
      kind: "decode",
    });
  });
});
