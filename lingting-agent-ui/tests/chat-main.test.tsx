/** 对话页渲染用例：初始状态不白屏，输入区可用。 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vite-plus/test";

import { ChatMain } from "@/pages/chat/ChatMain";

describe("ChatMain", () => {
  it("首屏渲染空状态与输入区", () => {
    render(<ChatMain />);

    expect(screen.getByText("输入消息开始对话")).toBeTruthy();
    expect(screen.getByPlaceholderText("输入消息，Enter 发送，Shift + Enter 换行")).toBeTruthy();
  });
});
