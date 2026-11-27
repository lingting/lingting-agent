/**
 * 技能抽屉用例（P5-015 的 `skill.web`）：清单、详情、开关与重载四条路径。
 *
 * 口径与 `tool-events.test.tsx` 一致：只断言抽屉对接口返回值的反应（清单内容、状态标签、
 * 动作入参与缓存替换），不做界面快照。动作后的清单来自 `api/skill/update` 的返回值，
 * 因此用例同时断言 `skillList` 只被调用一次。
 */

import type { SkillDetailVO, SkillVO } from "@lingting/agent-api";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { agentApi } from "@/api";
import { SkillDrawer } from "@/pages/chat/SkillDrawer";

vi.mock("@/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api")>();
  return {
    ...actual,
    agentApi: { skillGet: vi.fn(), skillList: vi.fn(), skillUpdate: vi.fn() },
  };
});

const api = agentApi as unknown as {
  skillGet: ReturnType<typeof vi.fn>;
  skillList: ReturnType<typeof vi.fn>;
  skillUpdate: ReturnType<typeof vi.fn>;
};

/** 一条清单项；只写用例关心的字段。 */
function skill(name: string, over: Partial<SkillVO> = {}): SkillVO {
  return {
    description: `${name} 的描述`,
    directory: `skills/${name}`,
    enabled: true,
    error: null,
    missingTools: [],
    name,
    referenceCount: 0,
    requiresTools: [],
    status: "READY",
    ...over,
  };
}

/** 一条详情。 */
function detail(base: SkillVO, over: Partial<SkillDetailVO> = {}): SkillDetailVO {
  return { ...base, body: "# 步骤\n\n1. 先读模板。", references: [], ...over };
}

/** 抽屉渲染：一个用例一个 QueryClient，互不共享缓存。 */
function renderDrawer() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <SkillDrawer onClose={() => undefined} open />
    </QueryClientProvider>,
  );
}

const BROKEN = skill("broken", {
  description: "",
  enabled: true,
  error: "skills/broken/SKILL.md:2: frontmatter 缺少 name",
  status: "INVALID",
});
const GAMMA = skill("gamma", { enabled: false, missingTools: ["pdf_render"] });
const ALPHA = skill("alpha");

describe("SkillDrawer", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // 自动清理只在全局 `test.globals` 打开时生效，这里与既有用例一样显式收尾。
  afterEach(cleanup);

  it("清单与接口一致，INVALID 与缺失工具都有标记", async () => {
    api.skillList.mockResolvedValue([ALPHA, BROKEN, GAMMA]);
    api.skillGet.mockResolvedValue(detail(ALPHA));
    renderDrawer();

    expect(await screen.findByText("alpha")).toBeTruthy();
    expect(screen.getByText("INVALID")).toBeTruthy();
    expect(screen.getByText(/缺 pdf_render/)).toBeTruthy();
    expect(screen.getByText("skills/broken")).toBeTruthy();
    // 禁用项置灰但仍在清单里：徽标与行都必须能看到它。
    expect(screen.getByText("gamma")).toBeTruthy();
  });

  it("INVALID 项详情显示可定位原因", async () => {
    api.skillList.mockResolvedValue([ALPHA, BROKEN]);
    api.skillGet.mockResolvedValue(detail(ALPHA));
    renderDrawer();

    fireEvent.click(await screen.findByText("broken"));

    expect(await screen.findByText("解析失败：不进索引，也不可加载")).toBeTruthy();
    expect(screen.getByText(BROKEN.error!)).toBeTruthy();
  });

  it("详情渲染正文与引用清单", async () => {
    api.skillList.mockResolvedValue([ALPHA]);
    api.skillGet.mockResolvedValue(
      detail(ALPHA, { references: ["skills/alpha/references/template.md"] }),
    );
    renderDrawer();

    fireEvent.click(await screen.findByText("alpha"));

    expect(await screen.findByText(/先读模板/)).toBeTruthy();
    expect(screen.getByText("skills/alpha/references/template.md")).toBeTruthy();
  });

  it("开关用返回值刷新清单，不再二次请求", async () => {
    api.skillList.mockResolvedValue([ALPHA]);
    api.skillUpdate.mockResolvedValue([{ ...ALPHA, enabled: false }]);
    renderDrawer();

    const toggle = await screen.findByRole("switch");
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(api.skillUpdate).toHaveBeenCalledWith({ action: "DISABLE", name: "alpha" });
    });
    await waitFor(() => {
      expect(screen.getByRole("switch").getAttribute("aria-checked")).toBe("false");
    });
    expect(api.skillList).toHaveBeenCalledTimes(1);
    expect(screen.getByText("禁用")).toBeTruthy();
  });

  it("重载后列表反映返回值", async () => {
    api.skillList.mockResolvedValue([ALPHA]);
    api.skillUpdate.mockResolvedValue([ALPHA, GAMMA]);
    renderDrawer();

    // antd 会在两个汉字的按钮文案里插一个空格，因此按正则找名字。
    fireEvent.click(await screen.findByRole("button", { name: /重\s*载/ }));

    await waitFor(() => {
      expect(screen.getByText("gamma")).toBeTruthy();
    });
    expect(api.skillUpdate).toHaveBeenCalledWith({ action: "RELOAD" });
    expect(api.skillList).toHaveBeenCalledTimes(1);
  });

  it("接口失败时原样展示服务端文案", async () => {
    api.skillList.mockResolvedValue([ALPHA]);
    api.skillUpdate.mockRejectedValue(new Error("技能不存在"));
    renderDrawer();

    fireEvent.click(await screen.findByRole("switch"));

    expect(await screen.findByText("技能不存在")).toBeTruthy();
  });
});
