import type {
  ClientCloseReason,
  ClientEvent,
  ClientStreamParams,
  RunEvent,
} from "@lingting/agent-api";

import type { AgentApiClient } from "./client";
import { subscribeClientEventStream } from "./clientStream";
import { ApiRequestError, errorMessageOf } from "./errors";

/** 心跳间隔：与服务端 `PING_INTERVAL` 一致（15s，D-P1.1-006）。 */
const PING_INTERVAL_MS = 15_000;

/** 服务端反注册后的固定 message（D-P1.1-005）：收到它必须重新注册。 */
export const NOT_REGISTERED = "客户端未注册，请重新连接";

/** 下行流推给界面的两类事件。 */
export type LinkEvent = { type: "run"; event: RunEvent } | { type: "closed"; reason: string };

/** 事件监听器。 */
export type LinkListener = (event: LinkEvent) => void;

/**
 * 客户端连接：注册 → 建立下行流 → 占用会话，并维持心跳（D-P1.1-001、RT-020）。
 *
 * 一个页面只有一条连接与一个 `clientId`：刷新页面时旧连接由服务端的 TTL 与流断标记回收，
 * 新注册不会残留旧记录。心跳与下行流都随 [`ChatLink::release`] 一起停掉。
 */
export class ChatLink {
  private readonly client: AgentApiClient;
  private readonly listener: LinkListener;

  /** 当前注册的客户端 ID。 */
  private clientId: string | null = null;

  /** 当前占用的会话 ID。 */
  private sessionId: string | null = null;

  /** 下行流的中断句柄。 */
  private abort: AbortController | null = null;

  /** 心跳定时器。 */
  private heartbeat: ReturnType<typeof setInterval> | null = null;

  /** 连接是否已经收尾：收尾后不再上报事件。 */
  private released = false;

  constructor(client: AgentApiClient, listener: LinkListener) {
    this.client = client;
    this.listener = listener;
  }

  /** 注册并建立下行流，返回 `clientId`。 */
  async connect(): Promise<string> {
    const registered = await this.client.clientCreate({});
    this.clientId = registered.clientId;
    this.released = false;
    this.startHeartbeat();
    const abort = new AbortController();
    this.abort = abort;
    void this.consume(registered.clientId, abort.signal);
    return registered.clientId;
  }

  /** 占用会话：换会话时先交还旧占用（一对一约束，D-P1.1-002）。 */
  async occupy(sessionId: string): Promise<void> {
    if (this.sessionId === sessionId) return;
    await this.releaseOccupancy();
    await this.client.occupancyCreate({ clientId: this.requireClientId(), sessionId });
    this.sessionId = sessionId;
  }

  /** 收尾：先交还占用、再反注册，最后停掉心跳与下行流。 */
  async release(): Promise<void> {
    this.released = true;
    this.stopHeartbeat();
    // 先交还占用（需要 clientId），再反注册；否则占用记录会留到 TTL 才回收。
    await this.releaseOccupancy().catch(() => undefined);
    const clientId = this.clientId;
    this.abort?.abort();
    this.abort = null;
    this.clientId = null;
    if (!clientId) return;
    await this.client.clientDelete({ clientId }).catch(() => undefined);
  }

  /** 取客户端 ID；未注册时给出可展示的错误。 */
  private requireClientId(): string {
    if (!this.clientId) throw new ApiRequestError("network", NOT_REGISTERED);
    return this.clientId;
  }

  /** 交还当前占用；没有占用时什么都不做。 */
  private async releaseOccupancy(): Promise<void> {
    const clientId = this.clientId;
    if (!this.sessionId || !clientId) return;
    this.sessionId = null;
    await this.client.occupancyDelete({ clientId });
  }

  /** 起心跳定时器。 */
  private startHeartbeat(): void {
    this.stopHeartbeat();
    this.heartbeat = setInterval(() => {
      void this.ping();
    }, PING_INTERVAL_MS);
  }

  /** 停心跳定时器：页面卸载时必须调用，否则定时器会跟着路由一直跑。 */
  private stopHeartbeat(): void {
    if (this.heartbeat !== null) {
      clearInterval(this.heartbeat);
      this.heartbeat = null;
    }
  }

  /** 一拍心跳；服务端反注册时把连接判为断（客户端据此重新注册）。 */
  private async ping(): Promise<void> {
    const clientId = this.clientId;
    if (!clientId) return;
    try {
      await this.client.clientUpdate({ clientId, kind: "PING" });
    } catch (failure) {
      if (errorMessageOf(failure) === NOT_REGISTERED) {
        this.fail(NOT_REGISTERED);
      }
    }
  }

  /** 消费下行流：`session_event` 交给界面，问询就地应答。 */
  private async consume(clientId: string, signal: AbortSignal): Promise<void> {
    const params: ClientStreamParams = { clientId };
    try {
      for await (const event of subscribeClientEventStream(this.client, params, signal)) {
        this.dispatch(event);
      }
    } catch (failure) {
      if (!signal.aborted) this.fail(errorMessageOf(failure));
      return;
    }
    if (!signal.aborted) this.fail("下行流已断开");
  }

  /** 一条下行事件分流。 */
  private dispatch(event: ClientEvent): void {
    switch (event.event) {
      case "session_event":
        if (event.sessionEvent) this.listener({ type: "run", event: event.sessionEvent });
        break;
      case "inquiry":
        // 问询应答不刷新 lastSeen（D-P1.1-006），只清掉待答状态。
        void this.client
          .clientUpdate({ clientId: this.requireClientId(), kind: "INQUIRY_ACK" })
          .catch(() => undefined);
        break;
      case "client_closed":
        this.fail(closeReasonText(event.closeReason));
        break;
    }
  }

  /** 连接已断：只上报一次，之后的收尾由 [`ChatLink::release`] 负责。 */
  private fail(reason: string): void {
    if (this.released) return;
    this.released = true;
    this.stopHeartbeat();
    this.listener({ type: "closed", reason });
  }
}

/** 关闭原因的可展示文案（与服务端固定文案一致）。 */
function closeReasonText(reason: ClientCloseReason | null | undefined): string {
  switch (reason) {
    case "SERVER_SHUTDOWN":
      return "服务正在关闭";
    case "HEARTBEAT_TIMEOUT":
    case "INQUIRY_TIMEOUT":
      return "连接已超时";
    case "CLIENT_CLOSED":
      return "客户端已断开";
    case "STREAM_CLOSED":
      return "下行流已断开";
    case "JOURNAL_ERROR":
      return "服务端写入事件失败";
    default:
      return "下行流已断开";
  }
}
