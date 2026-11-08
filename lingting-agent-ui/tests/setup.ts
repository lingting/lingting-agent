//! jsdom 环境下补齐 antd 与 @ant-design/x 依赖的浏览器 API。
//!
//! 只补「jsdom 未实现、组件库会直接调用」的部分；没有使用到的 API 不提前加垫片。

import { vi } from "vite-plus/test";

if (!window.matchMedia) {
  window.matchMedia = vi.fn((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })) as unknown as typeof window.matchMedia;
}

if (!globalThis.ResizeObserver) {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
}
