import { MessageOutlined } from "@ant-design/icons";

import { type MenuRouteDefinition, type StandaloneRouteDefinition } from "@lri";
import { ChatMain } from "@/pages/chat/ChatMain";

export const menuRoutes: readonly MenuRouteDefinition[] = [
  {
    component: ChatMain,
    icon: <MessageOutlined />,
    path: "chat",
    title: "对话",
  },
];

export const standaloneRoutes: readonly StandaloneRouteDefinition[] = [];
