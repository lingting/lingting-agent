import { AgentApiClient } from "./client";

export { AgentApiClient } from "./client";
export { ApiRequestError, errorMessageOf, isAborted } from "./errors";
export { subscribeRunEventStream } from "./runStream";

/** 全局唯一的 API 客户端：界面只通过生成的 SDK 方法访问服务端。 */
export const agentApi = new AgentApiClient();
