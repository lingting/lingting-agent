import type { ClientDecision } from "@lingting/agent-api";
import { Button, Card, Flex, Tag, Typography } from "antd";

import type { ToolCard, ToolStatus } from "./chatMessage";

/** 状态文案：一位用户在对话流里读得懂的说法。 */
const STATUS_TEXT: Record<ToolStatus, string> = {
  running: "调用中",
  ok: "完成",
  failed: "失败",
  cancelled: "已取消",
  confirm: "待确认",
  submitted: "等结果",
  unfinished: "未完成",
};

/** 状态颜色：沿用 antd 语义色，不新增设计 token。 */
const STATUS_COLOR: Record<ToolStatus, string | undefined> = {
  running: "processing",
  ok: "success",
  failed: "error",
  cancelled: "default",
  confirm: "warning",
  submitted: "processing",
  unfinished: "default",
};

/** 确认按钮的回调：只在 `confirm` 态可用，`submitted` 之后按钮禁用（D-P3-026）。 */
export type ToolConfirmHandler = (toolCallId: string, decision: ClientDecision) => void;

/** 工具调用卡片：一次调用一张，确认态追加参数摘要与两个按钮。 */
export function ToolCardView({
  card,
  onConfirm,
}: {
  card: ToolCard;
  onConfirm: ToolConfirmHandler;
}) {
  const confirmable = card.status === "confirm";
  return (
    <Card className="my-1 max-w-[560px]" size="small" styles={{ body: { padding: "8px 12px" } }}>
      <Flex align="center" gap={8} wrap>
        <Typography.Text strong>{card.name}</Typography.Text>
        <Tag color={STATUS_COLOR[card.status]}>{STATUS_TEXT[card.status]}</Tag>
        {card.durationMs && <Typography.Text type="secondary">{card.durationMs}ms</Typography.Text>}
      </Flex>
      {card.note && (
        <Typography.Paragraph className="mb-0 mt-1" type="secondary">
          {card.note}
        </Typography.Paragraph>
      )}
      {(confirmable || card.status === "submitted") && (
        <>
          <Typography.Paragraph className="mb-0 mt-1" ellipsis={{ rows: 2 }} type="secondary">
            {card.arguments || "（无参数）"}
          </Typography.Paragraph>
          <Flex className="mt-2" gap={8}>
            <Button
              disabled={!confirmable}
              size="small"
              type="primary"
              onClick={() => onConfirm(card.key, "APPROVE")}
            >
              允许
            </Button>
            <Button
              disabled={!confirmable}
              size="small"
              onClick={() => onConfirm(card.key, "REJECT")}
            >
              拒绝
            </Button>
          </Flex>
        </>
      )}
    </Card>
  );
}
