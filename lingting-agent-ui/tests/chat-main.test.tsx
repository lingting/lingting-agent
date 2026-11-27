/** 对话页渲染用例：初始状态不白屏，输入区与技能入口可用。 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { ChatMain } from "@/pages/chat/ChatMain";

/** 头部徽标读技能清单：用例里给空清单，既不联网也不依赖服务端。 */
vi.mock("@/api/skill", () => ({
  useSkillDetail: () => ({ data: undefined, error: null, isPending: false }),
  useSkillList: () => ({ data: [], error: null, isPending: false }),
  useSkillUpdate: () => ({ error: null, isPending: false, mutate: vi.fn() }),
}));

/** 头部徽标要 QueryClient：一层 Provider 与真实布局里的挂载点等价。 */
function renderChat() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ChatMain />
    </QueryClientProvider>,
  );
}

describe("ChatMain", () => {
  // 自动清理只在全局 `test.globals` 打开时生效，这里显式收尾。
  afterEach(cleanup);

  it("首屏渲染空状态与输入区", () => {
    renderChat();

    expect(screen.getByText("输入消息开始对话")).toBeTruthy();
    expect(screen.getByPlaceholderText("输入消息，Enter 发送，Shift + Enter 换行")).toBeTruthy();
  });

  it("头部有技能入口", () => {
    renderChat();

    expect(screen.getByRole("button", { name: /技能/ })).toBeTruthy();
  });
});
