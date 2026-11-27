import { ReadOutlined } from "@ant-design/icons";
import { Alert, Badge, Button, Empty, Flex, Typography } from "antd";
import { useState } from "react";

import { useSkillList } from "@/api/skill";

import { pausedNoticeOf } from "./chatMessage";
import { Composer } from "./Composer";
import { MessageList } from "./MessageList";
import { SkillDrawer } from "./SkillDrawer";
import { useChat } from "./useChat";

/** 对话页：发送消息 → 流式展示回复（含工具卡片与确认按钮）→ 可停止生成；失败以提示条呈现。 */
export function ChatMain() {
  const { cancel, clearError, confirm, error, items, paused, send, streaming } = useChat();
  const [input, setInput] = useState("");
  const [skillsOpen, setSkillsOpen] = useState(false);
  // 头部徽标读的是同一份清单缓存：抽屉打开与否都只请求一次。
  const skills = useSkillList();

  const handleSubmit = (content: string) => {
    setInput("");
    void send(content);
  };

  return (
    <Flex className="h-full" vertical>
      <Flex
        align="center"
        justify="space-between"
        style={{ borderBottom: "1px solid rgba(5, 5, 5, 0.06)", padding: "8px 16px" }}
      >
        <Typography.Text strong>对话</Typography.Text>
        <Badge count={(skills.data ?? []).length} size="small">
          <Button icon={<ReadOutlined />} onClick={() => setSkillsOpen(true)}>
            技能
          </Button>
        </Badge>
      </Flex>
      {error && <Alert banner closable message={error} type="error" onClose={clearError} />}
      {/* 中断态只提示、不阻止输入：发送一条消息就会先把上次没跑完的 Run 恢复（P6A-015）。 */}
      {paused && <Alert banner message={pausedNoticeOf(paused.reason)} type="warning" />}
      <div className="min-h-0 flex-1 overflow-hidden">
        {items.length > 0 ? (
          <MessageList items={items} onConfirm={confirm} />
        ) : (
          <Flex align="center" className="h-full" justify="center">
            <Empty description="输入消息开始对话" />
          </Flex>
        )}
      </div>
      <Composer
        onChange={setInput}
        onCancel={cancel}
        onSubmit={handleSubmit}
        streaming={streaming}
        value={input}
      />
      <SkillDrawer onClose={() => setSkillsOpen(false)} open={skillsOpen} />
    </Flex>
  );
}
