/**
 * 中断态的页面渲染（P6A-015 / 验收 I-5）。
 *
 * 只验证一件事：中断态显示成提示条（原因原样 + 「发送消息可继续」），输入区照旧可用，
 * 页面里**没有**恢复按钮——继续的方式就是发一条消息（D-P6-009）。
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { ChatMain } from "@/pages/chat/ChatMain";

/** 对话状态机由用例给出：这里只关心「中断态怎么渲染」。 */
const state = {
  paused: null as { runId: string; reason: string } | null,
};

vi.mock("@/pages/chat/useChat", () => ({
  useChat: () => ({
    cancel: vi.fn(),
    clearError: vi.fn(),
    confirm: vi.fn(),
    error: null,
    items: [],
    messages: [],
    paused: state.paused,
    send: vi.fn(),
    streaming: false,
  }),
}));

/** 头部徽标读技能清单：用例里给空清单，既不联网也不依赖服务端。 */
vi.mock("@/api/skill", () => ({
  useSkillDetail: () => ({ data: undefined, error: null, isPending: false }),
  useSkillList: () => ({ data: [], error: null, isPending: false }),
  useSkillUpdate: () => ({ error: null, isPending: false, mutate: vi.fn() }),
}));

/** 渲染对话页：头部徽标需要一层 QueryClient，与真实布局一致。 */
function renderChat() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ChatMain />
    </QueryClientProvider>,
  );
}

describe("中断态渲染", () => {
  beforeEach(() => {
    state.paused = null;
  });
  afterEach(cleanup);

  it("显示原因并提示发送消息可继续", () => {
    state.paused = { runId: "r-1", reason: "服务重启，执行中的 Run 已中断" };
    renderChat();

    expect(
      screen.getByText("已中断（发送消息可继续）：服务重启，执行中的 Run 已中断"),
    ).toBeTruthy();
  });

  it("中断态下输入框仍可用，且没有恢复按钮", () => {
    state.paused = { runId: "r-1", reason: "服务重启，执行中的 Run 已中断" };
    renderChat();

    const composer = screen.getByPlaceholderText("输入消息，Enter 发送，Shift + Enter 换行");
    expect(composer.hasAttribute("disabled")).toBe(false);
    expect(screen.queryByRole("button", { name: /恢复/ })).toBeNull();
  });

  it("没有中断态时不显示提示条", () => {
    renderChat();

    expect(screen.queryByText(/已中断（发送消息可继续）/)).toBeNull();
  });
});
