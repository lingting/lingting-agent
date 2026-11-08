import { Alert, Empty, Flex } from "antd";
import { useState } from "react";

import { Composer } from "./Composer";
import { MessageList } from "./MessageList";
import { useChat } from "./useChat";

/** 对话页：发送消息 → 流式展示回复 → 可停止生成；失败以提示条呈现。 */
export function ChatMain() {
  const { cancel, clearError, error, messages, send, streaming } = useChat();
  const [input, setInput] = useState("");

  const handleSubmit = (content: string) => {
    setInput("");
    void send(content);
  };

  return (
    <Flex className="h-full" vertical>
      {error && <Alert banner closable message={error} type="error" onClose={clearError} />}
      <div className="min-h-0 flex-1 overflow-hidden">
        {messages.length > 0 ? (
          <MessageList messages={messages} />
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
    </Flex>
  );
}
