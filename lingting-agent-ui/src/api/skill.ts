import type { SkillAction } from "@lingting/agent-api";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { agentApi } from "./index";

/** 清单查询键：动作与重载都写回同一个键，界面因此只有一份清单缓存。 */
export const SKILL_LIST_KEY = ["skill", "list"] as const;

/** 详情缓存的键前缀：动作成功后整体失效，避免正文与启用状态对不上。 */
export const SKILL_DETAIL_PREFIX = ["skill", "detail"] as const;

/** 一条技能的详情查询键：按名字分开缓存，切换选中项不必重取已看过的正文。 */
export function skillDetailKey(name: string) {
  return [...SKILL_DETAIL_PREFIX, name] as const;
}

/** 技能清单（含 `INVALID` 与已禁用项）。 */
export function useSkillList() {
  return useQuery({ queryFn: () => agentApi.skillList(), queryKey: SKILL_LIST_KEY });
}

/** 一条技能的详情；`name` 为空时不发请求。 */
export function useSkillDetail(name: string | null) {
  return useQuery({
    enabled: name !== null,
    queryFn: () => agentApi.skillGet({ name: name ?? "" }),
    queryKey: skillDetailKey(name ?? ""),
  });
}

/**
 * 启停与重载：服务端返回动作完成后的**整张清单**，直接写回缓存，不再二次请求（D-P5-015）。
 *
 * 详情缓存整体失效：禁用项的服务端正文是空串，留着旧正文会出现「禁用却仍显示全文」。
 */
export function useSkillUpdate() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (po: { action: SkillAction; name?: string }) => agentApi.skillUpdate(po),
    onSuccess: (list) => {
      queryClient.setQueryData(SKILL_LIST_KEY, list);
      void queryClient.removeQueries({ queryKey: SKILL_DETAIL_PREFIX });
    },
  });
}
