/** 接口失败的类别：区分连不上、状态码错误、响应解析失败与用户主动中断。 */
export type ApiFailureKind = "network" | "http" | "decode" | "aborted";

/** 统一的接口调用错误；`message` 是可直接展示给用户的文案。 */
export class ApiRequestError extends Error {
  readonly kind: ApiFailureKind;

  /** 服务端错误码，与 HTTP 状态码同值；非 HTTP 错误时为空。 */
  readonly code?: number;

  constructor(kind: ApiFailureKind, message: string, code?: number) {
    super(message);
    this.name = "ApiRequestError";
    this.kind = kind;
    this.code = code;
  }
}

/** 把任意失败转成一行可读文案，不把堆栈暴露给用户。 */
export function errorMessageOf(error: unknown): string {
  if (error instanceof ApiRequestError) return error.message;
  if (error instanceof Error && error.message) return error.message;
  return "请求失败，请稍后重试";
}

/** 是否为主动中断（停止生成）。 */
export function isAborted(error: unknown): boolean {
  if (error instanceof ApiRequestError) return error.kind === "aborted";
  return error instanceof Error && error.name === "AbortError";
}
